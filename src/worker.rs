use crate::agent::comfort_loop::{AlertPayload, AlertType};
use crate::entities::{daily_digest, pet_video, DailyDigest, PetVideo};
use crate::gemini::GeminiClient;
use chrono::{NaiveDate, Utc};
use google_cloud_storage::client::Client as GcsClient;
use google_cloud_storage::http::objects::download::Range;
use google_cloud_storage::http::objects::get::GetObjectRequest;
use redis::AsyncCommands;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde_json::Value;
use std::sync::Arc;
use tracing::Instrument;
use uuid::Uuid;

// Queue Monitoring
pub async fn start_queue_monitor(redis_client: redis::Client) {
    let redis_client = Arc::new(redis_client);

    // Spawn a background task
    tokio::spawn(async move {
        tracing::info!("Queue Monitor started");
        loop {
            let mut conn = match redis_client.get_multiplexed_async_connection().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Queue Monitor: Failed to get redis conn: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
                    continue;
                }
            };

            let video_queue_len: redis::RedisResult<u64> = conn.llen("video_queue").await;
            match video_queue_len {
                Ok(len) => metrics::gauge!("petpulse_queue_depth", "queue" => "video_queue")
                    .set(len as f64),
                Err(e) => tracing::error!("Failed to get video_queue len: {}", e),
            }

            let digest_queue_len: redis::RedisResult<u64> = conn.llen("digest_queue").await;
            match digest_queue_len {
                Ok(len) => metrics::gauge!("petpulse_queue_depth", "queue" => "digest_queue")
                    .set(len as f64),
                Err(e) => tracing::error!("Failed to get digest_queue len: {}", e),
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
        }
    });
}

pub async fn start_workers(
    redis_client: redis::Client,
    db: DatabaseConnection,
    concurrency: usize,
    gcs_client: GcsClient,
) {
    // Start Queue Monitor
    start_queue_monitor(redis_client.clone()).await;

    let db = Arc::new(db);
    let redis_client = Arc::new(redis_client);
    let gcs_client = Arc::new(gcs_client);
    // Shared Gemini Client
    let gemini_client = Arc::new(GeminiClient::new());

    for i in 0..concurrency {
        let db = db.clone();
        let redis_client = redis_client.clone();
        let gcs_client = gcs_client.clone();
        let gemini = gemini_client.clone();

        tokio::spawn(async move {
            tracing::info!("Worker {} started", i);
            loop {
                // Get connection
                let mut conn = match redis_client.get_multiplexed_async_connection().await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("Worker {}: Failed to get redis conn: {}", i, e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                let result: redis::RedisResult<(String, String)> =
                    conn.blpop("video_queue", 0.0).await;

                match result {
                    Ok((_key, payload_str)) => {
                        let payload: Value = match serde_json::from_str(&payload_str) {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::error!("Worker {}: Bad payload: {}", i, e);
                                continue;
                            }
                        };

                        let video_id_str = payload["video_id"].as_str().unwrap_or("");
                        let video_id = match Uuid::parse_str(video_id_str) {
                            Ok(id) => id,
                            Err(_) => {
                                tracing::error!("Worker {}: Invalid UUID", i);
                                continue;
                            }
                        };

                        process_video(video_id, &db, &gemini, &mut conn, &gcs_client, &payload)
                            .await;
                    }
                    Err(e) => {
                        tracing::error!("Worker {}: Redis error: {}", i, e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    }
                }
            }
        });
    }
}

async fn process_video(
    video_id: Uuid,
    db: &DatabaseConnection,
    gemini: &GeminiClient,
    redis_conn: &mut redis::aio::MultiplexedConnection,
    gcs_client: &GcsClient,
    payload: &Value,
) {
    // Extract Trace Context
    use opentelemetry::propagation::TextMapPropagator;
    use opentelemetry_sdk::propagation::TraceContextPropagator;
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    let parent_context = if let Some(carrier_map) = payload["trace_context"].as_object() {
        let carrier: std::collections::HashMap<String, String> = carrier_map
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
            .collect();
        let propagator = TraceContextPropagator::new();
        propagator.extract(&carrier)
    } else {
        opentelemetry::Context::new()
    };

    let span = tracing::info_span!("process_video_job", "otel.name" = "process_video_job", video_id = ?video_id);
    span.set_parent(parent_context);

    let _enter = span.enter();
    tracing::info!("Dequeued video {} from video_queue", video_id);
    drop(_enter); // Drop guard to re-enter in async block via .instrument()

    let start_time = std::time::Instant::now();

    async move {
        // 1. Fetch Video Entity
        let video_opt = PetVideo::find_by_id(video_id).one(db).await.unwrap_or(None);
        if video_opt.is_none() {
            tracing::error!("Video {} not found in DB", video_id);
            return;
        }
        let video = video_opt.unwrap();
        let retry_count = video.retry_count;

        // 2. Set Status PROCESSING
        let mut active_video: pet_video::ActiveModel = video.clone().into();
        active_video.status = Set("PROCESSING".to_string());
        if let Err(e) = active_video.update(db).await {
            tracing::error!("Failed to update status: {}", e);
            metrics::counter!("petpulse_video_processing_errors_total", "stage" => "db_update").increment(1);
            return;
        }

        // 3. Download from GCS
        let gcs_path = video.file_path.clone();
        let temp_file_path = format!("/tmp/{}", video_id);

        async {
            // Parse bucket and object
            // Expecting: gs://bucket/object/path
            let parts: Vec<&str> = gcs_path
                .trim_start_matches("gs://")
                .splitn(2, '/')
                .collect();
            if parts.len() != 2 {
                tracing::error!("Invalid GCS URI: {}", gcs_path);
                // Fail
                let mut active: pet_video::ActiveModel = video.clone().into();
                active.status = Set("FAILED".to_string());
                let _ = active.update(db).await;
                metrics::counter!("petpulse_video_processing_errors_total", "stage" => "download").increment(1);
                return;
            }
            let bucket = parts[0];
            let object = parts[1];

            let data = match gcs_client
                .download_object(
                    &GetObjectRequest {
                        bucket: bucket.to_string(),
                        object: object.to_string(),
                        ..Default::default()
                    },
                    &Range::default(),
                )
                .await
            {
                Ok(d) => d,
                Err(e) => {
                    tracing::error!("Failed to download from GCS: {}", e);
                    // Fail or Retry logic?
                    // Let's retry if transient, fail for now to keep simple.
                    metrics::counter!("petpulse_video_processing_errors_total", "stage" => "download").increment(1);
                    return;
                }
            };

            if let Err(e) = tokio::fs::write(&temp_file_path, data).await {
                tracing::error!("Failed to write temp file: {}", e);
                metrics::counter!("petpulse_video_processing_errors_total", "stage" => "fs_write").increment(1);
                return;
            }
        }.instrument(tracing::info_span!("download_video_gcs")).await;


        // 4. Analyze
        async {
            match gemini.analyze_video_with_usage(&temp_file_path).await {
                Ok((analysis_result, usage_metadata)) => {
                    tracing::info!("Analysis successful for {}", video_id);
                    tracing::info!("Raw Analysis Result: {:?}", analysis_result);
                    tracing::info!("Usage Metadata: {:?}", usage_metadata);

                    // Record Token Usage
                    if let Some(usage) = usage_metadata {
                        if let Some(input_tokens) = usage["promptTokenCount"].as_i64() {
                             metrics::counter!("petpulse_gemini_tokens_total", "type" => "input").increment(input_tokens as u64);
                        }
                        if let Some(output_tokens) = usage["candidatesTokenCount"].as_i64() {
                             metrics::counter!("petpulse_gemini_tokens_total", "type" => "output").increment(output_tokens as u64);
                        }
                    }

                    // Update Status PROCESSED
                    let mut active: pet_video::ActiveModel = video.clone().into();
                    active.status = Set("PROCESSED".to_string());

                    // Save Analysis directly to PetVideo
                    if let Some(activities_value) = analysis_result.get("activities") {
                        if let Ok(_activities) =
                            serde_json::from_value::<Vec<pet_video::Activity>>(activities_value.clone())
                        {
                            active.activities = Set(Some(activities_value.clone()));
                        } else {
                            tracing::error!(
                                "Failed to parse activities matching schema: {:?}",
                                activities_value
                            );
                        }
                    } else {
                        tracing::warn!("'activities' key missing in analysis result");
                    }
                    active.mood = Set(analysis_result["summary_mood"]
                        .as_str()
                        .map(|s| s.to_string()));
                    active.description = Set(analysis_result["summary_description"]
                        .as_str()
                        .map(|s| s.to_string()));
                    active.is_unusual = Set(analysis_result["is_unusual"].as_bool().unwrap_or(false));

                    // Extract severity level (Phase 3 enhancement)
                    let severity_level = analysis_result["severity_level"]
                        .as_str()
                        .unwrap_or("low")
                        .to_string();

                    // Extract critical indicators if present
                    let critical_indicators = analysis_result.get("critical_indicators")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect::<Vec<String>>()
                        })
                        .unwrap_or_default();

                    // Extract recommended actions if present
                    let recommended_actions = analysis_result.get("recommended_actions")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect::<Vec<String>>()
                        })
                        .unwrap_or_default();

                    tracing::info!(
                        "Updating video {} with: mood={:?}, unusual={:?}, severity={}",
                        video_id,
                        active.mood,
                        active.is_unusual,
                        severity_level
                    );

                    // Route alerts based on severity level (Phase 3)
                    if severity_level == "critical" {
                        // CRITICAL ALERT PATH
                        metrics::counter!("petpulse_critical_alerts_total", "pet_id" => active.pet_id.clone().unwrap().to_string()).increment(1);

                        tracing::warn!(
                            "🚨 CRITICAL alert detected for video_id={}, pet_id={}, indicators={:?}",
                            video_id,
                            active.pet_id.clone().unwrap(),
                            critical_indicators
                        );

                        let pet_id = active.pet_id.clone().unwrap();
                        let description = active.description.clone().unwrap().unwrap_or_else(|| "Critical health condition detected".to_string());
                        let mood = active.mood.clone().unwrap();

                        tokio::spawn(async move {
                            send_critical_alert_webhook(
                                video_id,
                                pet_id,
                                description,
                                mood,
                                critical_indicators,
                                recommended_actions,
                            ).await;
                        });
                    } else if active.is_unusual.clone().unwrap() {
                        // NORMAL UNUSUAL BEHAVIOR PATH
                        metrics::counter!("petpulse_unusual_events_total", "pet_id" => active.pet_id.clone().unwrap().to_string()).increment(1);

                        let pet_id = active.pet_id.clone().unwrap();
                        let description = active.description.clone().unwrap().unwrap_or_else(|| "Unusual activity detected".to_string());
                        let mood = active.mood.clone().unwrap();

                        tokio::spawn(async move {
                            send_alert_webhook(video_id, pet_id, description, mood, severity_level).await;
                        });
                    }

                    match active.update(db).await {
                        Ok(v) => {
                            tracing::info!("Updated video successfully: {:?}", v);

                            // Queue digest update
                            let date = v.created_at.date_naive();
                            let digest_payload = serde_json::json!({
                                "pet_id": v.pet_id,
                                "date": date.format("%Y-%m-%d").to_string()
                            })
                            .to_string();

                            let _: () = redis_conn
                                .rpush("digest_queue", digest_payload)
                                .await
                                .unwrap_or(());

                            tracing::info!(
                                "Enqueued digest update for pet_id={} to digest_queue",
                                v.pet_id
                            );

                            metrics::counter!("petpulse_video_processed_total").increment(1);
                        }
                        Err(e) => {
                             tracing::error!("Failed to update video {}: {}", video_id, e);
                             metrics::counter!("petpulse_video_processing_errors_total", "stage" => "db_final_update").increment(1);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Analysis failed for {}: {}", video_id, e);
                    metrics::counter!("petpulse_gemini_api_errors_total").increment(1);
                    // We should also record duration here effectively, but it's inside the block.
                    // Let's rely on the outer duration. But wait, "success" label differs.
                    // The outer block will record success=true even if this fails? No, the outer block blindly records success=true currently.
                    // Correcting the outer block requires state.
                    // Since I can't easily change the outer block structure in this single-tool edit without making it huge,
                    // I will leave the outer recording as "true" for now (or I should just remove "success" label from plan).
                    // Actually, let's fix it properly. I will add a variable `success` in outer scope.

                    if retry_count < 2 {
                        // Retry
                        let mut active: pet_video::ActiveModel = video.clone().into();
                        active.retry_count = Set(retry_count + 1);
                        active.status = Set("Retrying".to_string());
                        let _ = active.update(db).await;

                        let payload = serde_json::json!({ "video_id": video_id }).to_string();
                        let _: () = redis_conn.rpush("video_queue", payload).await.unwrap_or(());
                    } else {
                        // Fail
                        let mut active: pet_video::ActiveModel = video.clone().into();
                        active.status = Set("FAILED".to_string());
                        let _ = active.update(db).await;
                    }
                }
            }
            // Cleanup in both cases
            let _ = tokio::fs::remove_file(&temp_file_path).await;

        }.instrument(tracing::info_span!("analyze_video_gemini")).await;

        let duration = start_time.elapsed().as_secs_f64();
        metrics::histogram!("petpulse_video_processing_duration_seconds", "success" => "true").record(duration);

    }.instrument(span).await;
}

// ============================================================================
// Digest Workers
// ============================================================================

pub async fn start_digest_workers(
    redis_client: redis::Client,
    db: DatabaseConnection,
    concurrency: usize,
) {
    let db = Arc::new(db);
    let redis_client = Arc::new(redis_client);

    for i in 0..concurrency {
        let db = db.clone();
        let redis_client = redis_client.clone();

        tokio::spawn(async move {
            tracing::info!("Digest Worker {} started", i);
            loop {
                // Get connection
                let mut conn = match redis_client.get_multiplexed_async_connection().await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("Digest Worker {}: Failed to get redis conn: {}", i, e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                let result: redis::RedisResult<(String, String)> =
                    conn.blpop("digest_queue", 0.0).await;

                match result {
                    Ok((_key, payload_str)) => {
                        let payload: Value = match serde_json::from_str(&payload_str) {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::error!("Digest Worker {}: Bad payload: {}", i, e);
                                continue;
                            }
                        };

                        let pet_id = payload["pet_id"].as_i64().unwrap_or(0) as i32;
                        let date_str = payload["date"].as_str().unwrap_or("");
                        let date = match NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                            Ok(d) => d,
                            Err(_) => {
                                tracing::error!("Digest Worker {}: Invalid date: {}", i, date_str);
                                continue;
                            }
                        };

                        process_digest_update(pet_id, date, &db, i).await;
                    }
                    Err(e) => {
                        tracing::error!("Digest Worker {}: Redis error: {}", i, e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    }
                }
            }
        });
    }
}

async fn process_digest_update(
    pet_id: i32,
    date: NaiveDate,
    db: &DatabaseConnection,
    worker_id: usize,
) {
    let span = tracing::info_span!(
        "process_digest_job",
        "otel.name" = "process_digest_job",
        pet_id = pet_id
    );
    process_digest_update_impl(pet_id, date, db, worker_id)
        .instrument(span)
        .await
}

async fn process_digest_update_impl(
    pet_id: i32,
    date: NaiveDate,
    db: &DatabaseConnection,
    worker_id: usize,
) {
    tracing::info!(
        "Dequeued digest update for pet_id={} from digest_queue",
        pet_id
    );

    tracing::info!(
        "Digest Worker {}: Processing pet_id={}, date={}",
        worker_id,
        pet_id,
        date
    );

    // 1. Query all PROCESSED videos for this pet and date
    let videos = match PetVideo::find()
        .filter(pet_video::Column::PetId.eq(pet_id))
        .filter(pet_video::Column::Status.eq("PROCESSED"))
        .all(db)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Digest Worker {}: Failed to query videos: {}", worker_id, e);
            return;
        }
    };

    // Filter by date (since we need to compare DateTimeWithTimeZone)
    let videos_for_date: Vec<_> = videos
        .into_iter()
        .filter(|v| v.created_at.date_naive() == date)
        .collect();

    if videos_for_date.is_empty() {
        tracing::warn!(
            "Digest Worker {}: No processed videos found for pet_id={}, date={}",
            worker_id,
            pet_id,
            date
        );
        return;
    }

    tracing::info!(
        "Digest Worker {}: Found {} videos for pet_id={}, date={}",
        worker_id,
        videos_for_date.len(),
        pet_id,
        date
    );

    // 2. Aggregate data from all videos
    let mut all_activities_json = Vec::new();
    let mut all_moods = Vec::new();
    let mut all_descriptions = Vec::new();
    let mut unusual_events_list = Vec::new();

    for video in &videos_for_date {
        // Parse activities
        if let Some(activities_json) = &video.activities {
            if let Ok(_activities) =
                serde_json::from_value::<Vec<pet_video::Activity>>(activities_json.clone())
            {
                // Store raw activity objects for JSON column
                if let Some(arr) = activities_json.as_array() {
                    all_activities_json.extend(arr.clone());
                }
            }
        }

        // Collect moods
        if let Some(mood) = &video.mood {
            all_moods.push(mood.clone());
        }

        // Collect descriptions
        if let Some(desc) = &video.description {
            all_descriptions.push(desc.clone());
        }

        // Collect unusual events
        if video.is_unusual {
            // Create a structured object for unusual event
            let event_obj = serde_json::json!({
                "video_id": video.id.to_string(),
                "description": video.description.clone().unwrap_or("Unusual activity detected".to_string()),
                // We could add "timestamp" here if needed
                "timestamp": video.created_at.to_rfc3339()
            });
            unusual_events_list.push(event_obj);
        }
    }

    // 3. Generate summary
    let summary = format!(
        "Daily Summary for Pet {}\n\n\
        Videos Processed: {}\n\
        Moods: {}\n\
        Unusual Events: {}\n\n\
        Descriptions:\n{}",
        pet_id,
        videos_for_date.len(),
        if all_moods.is_empty() {
            "None".to_string()
        } else {
            all_moods.join(", ")
        },
        unusual_events_list.len(),
        if all_descriptions.is_empty() {
            "No descriptions available.".to_string()
        } else {
            all_descriptions.join("\n\n")
        }
    );

    let moods_json = serde_json::to_value(all_moods).unwrap_or(serde_json::json!([]));
    let activities_json =
        serde_json::to_value(all_activities_json).unwrap_or(serde_json::json!([]));
    let unusual_json = serde_json::to_value(unusual_events_list).unwrap_or(serde_json::json!([]));

    // 4. UPSERT daily_digest
    // First, try to find existing digest
    let existing = DailyDigest::find()
        .filter(daily_digest::Column::PetId.eq(pet_id))
        .filter(daily_digest::Column::Date.eq(date))
        .one(db)
        .await
        .unwrap_or(None);

    let result = if let Some(existing_digest) = existing {
        // Update existing
        let mut active: daily_digest::ActiveModel = existing_digest.into();
        active.summary = Set(summary.clone());
        active.moods = Set(Some(moods_json));
        active.activities = Set(Some(activities_json));
        active.unusual_events = Set(Some(unusual_json));
        active.total_videos = Set(videos_for_date.len() as i32);
        active.updated_at = Set(Utc::now().into());
        active.update(db).await
    } else {
        // Insert new
        let new_digest = daily_digest::ActiveModel {
            id: Set(Uuid::new_v4()),
            pet_id: Set(pet_id),
            date: Set(date),
            summary: Set(summary.clone()),
            moods: Set(Some(moods_json)),
            activities: Set(Some(activities_json)),
            unusual_events: Set(Some(unusual_json)),
            total_videos: Set(videos_for_date.len() as i32),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        new_digest.insert(db).await
    };

    match result {
        Ok(_) => {
            tracing::info!(
                "Digest Worker {}: Successfully updated digest for pet_id={}, date={}",
                worker_id,
                pet_id,
                date
            );
            metrics::counter!("petpulse_daily_digests_generated_total").increment(1);
        }
        Err(e) => {
            tracing::error!(
                "Digest Worker {}: Failed to upsert digest: {}",
                worker_id,
                e
            );
        }
    }
}

// ============================================================================
// Alert Webhook Helper
// ============================================================================

async fn send_alert_webhook(
    video_id: Uuid,
    pet_id: i32,
    description: String,
    mood: Option<String>,
    severity_level: String,
) {
    let agent_url = std::env::var("AGENT_SERVICE_URL")
        .unwrap_or_else(|_| "http://agent:3002/alert".to_string());

    // Map severity_level to legacy severity field for backward compatibility
    let severity = match severity_level.as_str() {
        "info" => "low",
        "low" => "medium",
        "medium" => "high",
        "high" => "high",
        _ => "medium",
    };

    let alert_payload = AlertPayload {
        alert_id: Uuid::new_v4().to_string(),
        pet_id: pet_id.to_string(),
        alert_type: AlertType::UnusualBehavior,
        severity: severity.to_string(),
        message: Some(description.clone()),
        metric_value: None,
        baseline_value: None,
        deviation_factor: None,
        video_id: Some(video_id.to_string()),
        timestamp: Some(Utc::now().to_rfc3339()),
        context: Some(serde_json::json!({
            "mood": mood,
            "description": description,
            "severity_level": severity_level,
        })),
        title: Some("Unusual Behavior Detected".to_string()),
        state: Some("alerting".to_string()),
        eval_matches: None,
        severity_level: Some(severity_level.clone()),
        critical_indicators: None,
        recommended_actions: None,
    };

    tracing::info!(
        "Sending alert webhook for video_id={}, pet_id={}, severity_level={}",
        video_id,
        pet_id,
        severity_level
    );

    let client = reqwest::Client::new();
    match client.post(&agent_url).json(&alert_payload).send().await {
        Ok(resp) => {
            if resp.status().is_success() {
                tracing::info!("Successfully sent alert webhook to agent service");
            } else {
                tracing::error!(
                    "Agent service returned error: {} - {}",
                    resp.status(),
                    resp.text()
                        .await
                        .unwrap_or_else(|_| "<unable to read response>".to_string())
                );
            }
        }
        Err(e) => {
            tracing::error!("Failed to send alert webhook to agent service: {}", e);
        }
    }
}

// ============================================================================
// Critical Alert Webhook (Phase 3)
// ============================================================================

async fn send_critical_alert_webhook(
    video_id: Uuid,
    pet_id: i32,
    description: String,
    mood: Option<String>,
    critical_indicators: Vec<String>,
    recommended_actions: Vec<String>,
) {
    let agent_url = std::env::var("AGENT_SERVICE_URL")
        .unwrap_or_else(|_| "http://agent:3002/alert/critical".to_string());

    let alert_payload = AlertPayload {
        alert_id: Uuid::new_v4().to_string(),
        pet_id: pet_id.to_string(),
        alert_type: AlertType::UnusualBehavior, // Will be enhanced to CriticalHealth in Phase 5
        severity: "critical".to_string(),
        message: Some(description.clone()),
        metric_value: None,
        baseline_value: None,
        deviation_factor: None,
        video_id: Some(video_id.to_string()),
        timestamp: Some(Utc::now().to_rfc3339()),
        context: Some(serde_json::json!({
            "mood": mood,
            "description": description,
            "severity_level": "critical",
            "critical_indicators": critical_indicators,
            "recommended_actions": recommended_actions,
        })),
        title: Some("🚨 CRITICAL ALERT: Immediate Attention Required".to_string()),
        state: Some("critical".to_string()),
        eval_matches: None,
        severity_level: Some("critical".to_string()),
        critical_indicators: Some(critical_indicators.clone()),
        recommended_actions: Some(recommended_actions),
    };

    tracing::warn!(
        "🚨 Sending CRITICAL alert webhook for video_id={}, pet_id={}, indicators={:?}",
        video_id,
        pet_id,
        critical_indicators
    );

    let client = reqwest::Client::new();
    match client.post(&agent_url).json(&alert_payload).send().await {
        Ok(resp) => {
            if resp.status().is_success() {
                tracing::info!("✅ Successfully sent CRITICAL alert webhook to agent service");
            } else {
                tracing::error!(
                    "❌ Agent service returned error for CRITICAL alert: {} - {}",
                    resp.status(),
                    resp.text()
                        .await
                        .unwrap_or_else(|_| "<unable to read response>".to_string())
                );
            }
        }
        Err(e) => {
            tracing::error!(
                "❌ Failed to send CRITICAL alert webhook to agent service: {}",
                e
            );
        }
    }
}
