use crate::entities::{daily_digest, pet, pet_video, DailyDigest, PetVideo};
use axum::{
    extract::{Extension, Multipart, Path, Query},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use chrono::Utc;
use google_cloud_storage::client::Client as GcsClient;
use google_cloud_storage::http::objects::upload::{UploadObjectRequest, UploadType};
use redis::AsyncCommands;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct GenerateDigestRequest {
    date: Option<chrono::NaiveDate>,
}

pub async fn upload_video(
    Path(pet_id): Path<i32>,
    Extension(db): Extension<DatabaseConnection>,
    Extension(redis_client): Extension<redis::Client>,
    Extension(gcs_client): Extension<GcsClient>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let bucket_name = std::env::var("GCS_BUCKET_NAME").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "GCS_BUCKET_NAME not set".to_string(),
        )
    })?;

    // 1. Process Multipart
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();

        if name == "video" {
            let file_name = field.file_name().unwrap_or("video.mp4").to_string();
            let data = field
                .bytes()
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            // Validate size
            if data.len() > 500 * 1024 * 1024 {
                // 500MB
                return Err((StatusCode::PAYLOAD_TOO_LARGE, "File too large".to_string()));
            }

            // GCS Upload
            let file_uuid = Uuid::new_v4();
            let ext = std::path::Path::new(&file_name)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("mp4");
            let object_name = format!("uploads/{}/{}.{}", pet_id, file_uuid, ext);
            let mime_type = mime_guess::from_path(&file_name)
                .first_or_octet_stream()
                .to_string();

            let upload_type =
                UploadType::Simple(google_cloud_storage::http::objects::upload::Media {
                    name: object_name.clone().into(),
                    content_type: mime_type.into(),
                    content_length: Some(data.len() as u64),
                });

            let _uploaded = gcs_client
                .upload_object(
                    &UploadObjectRequest {
                        bucket: bucket_name.clone(),
                        ..Default::default()
                    },
                    data,
                    &upload_type,
                )
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("GCS Upload Failed: {}", e),
                    )
                })?;

            let gcs_path = format!("gs://{}/{}", bucket_name, object_name);

            // 2. Create PetVideo Record
            let now = Utc::now().with_timezone(&chrono::FixedOffset::east_opt(0).unwrap());
            let pet_video = pet_video::ActiveModel {
                id: Set(file_uuid),
                pet_id: Set(pet_id),
                file_path: Set(gcs_path.clone()),
                status: Set("PENDING".to_string()),
                retry_count: Set(0),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            };

            let _saved_video = pet_video.insert(&db).await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("DB Error: {}", e),
                )
            })?;

            tracing::Span::current()
                .record("table", "pet_videos")
                .record("action", "upload")
                .record("video_id", file_uuid.to_string())
                .record("pet_id", pet_id)
                .record("business_event", "Video uploaded to GCS and recorded in DB");

            metrics::counter!("petpulse_videos_uploaded_total", "pet_id" => pet_id.to_string())
                .increment(1);
            metrics::gauge!("petpulse_videos_total").increment(1.0);

            // Increment per-pet count
            let db_clone = db.clone();
            tokio::spawn(async move {
                crate::metrics::increment_pet_videos(&db_clone, pet_id).await;
            });

            // 3. Push to Redis
            let mut conn = redis_client
                .get_multiplexed_async_connection()
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Redis Conn Error: {}", e),
                    )
                })?;

            // Propagate Trace Context
            use opentelemetry::propagation::TextMapPropagator;
            use opentelemetry_sdk::propagation::TraceContextPropagator;
            use tracing_opentelemetry::OpenTelemetrySpanExt;

            let mut carrier = std::collections::HashMap::new();
            let propagator = TraceContextPropagator::new();
            let context = tracing::Span::current().context();
            propagator.inject_context(&context, &mut carrier);

            let payload = serde_json::json!({
                "video_id": file_uuid,
                "trace_context": carrier
            })
            .to_string();

            let _: () = conn.rpush("video_queue", payload).await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Redis Push Error: {}", e),
                )
            })?;

            tracing::info!("Enqueued video {} to video_queue", file_uuid);

            return Ok(Json(json!({
                "status": "queued",
                "video_id": file_uuid
            })));
        }
    }

    Err((StatusCode::BAD_REQUEST, "No video field found".to_string()))
}

pub async fn generate_daily_digest(
    Extension(db): Extension<DatabaseConnection>,
    Json(payload): Json<GenerateDigestRequest>,
    // Triggered manually or by cron.
    // Payload can specify params, but let's assume we want to process ALL eligible digests for a given date?
    // Or maybe for specific pet. Let's start with polling all. But for this endpoint, maybe for a specific pet?
    // User requested "server auto-triggers". Let's assume this endpoint triggers for ALL users or we pass params.
    // For simplicity, let's make this endpoint: trigger digest for All Pets for a specific Date (default today)
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let date = payload.date.unwrap_or_else(|| Utc::now().date_naive());

    // We need to group videos by PetId.
    // Since we don't have GROUP BY easily in ORM, let's fetch all processed videos for the date.
    // Ideally query pets then videos.

    use sea_orm::QuerySelect;

    // 1. Find all videos processed on this date
    // Note: 'created_at' is DateTimeWithTimeZone. We need to cast or filter by range.
    let start_of_day = date
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .with_timezone(&chrono::FixedOffset::east_opt(0).unwrap());
    let end_of_day = date
        .and_hms_opt(23, 59, 59)
        .unwrap()
        .and_utc()
        .with_timezone(&chrono::FixedOffset::east_opt(0).unwrap());

    let videos = PetVideo::find()
        .filter(pet_video::Column::Status.eq("PROCESSED"))
        .filter(pet_video::Column::CreatedAt.gte(start_of_day))
        .filter(pet_video::Column::CreatedAt.lte(end_of_day))
        .all(&db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("DB Query Error: {}", e),
            )
        })?;

    // Group by PetID
    let mut pet_videos_map: std::collections::HashMap<i32, Vec<pet_video::Model>> =
        std::collections::HashMap::new();
    for v in videos {
        pet_videos_map.entry(v.pet_id).or_default().push(v);
    }

    let mut generated_count = 0;

    for (pet_id, clips) in pet_videos_map {
        // Aggregate
        let mut summaries = String::new();
        let mut activities_list = Vec::new();
        let mut unusual_events = Vec::new();
        let mut mood_counts = std::collections::HashMap::new();

        for clip in &clips {
            // Description as Summary
            if let Some(s) = &clip.description {
                summaries.push_str(&format!("- {}\n", s));
            }

            // Activities
            if let Some(activities_val) = &clip.activities {
                if let Some(acts) = activities_val.as_array() {
                    // It's a list of objects now
                    for a in acts {
                        // The new schema for Activity: { activity, mood, description, ... }
                        // We probably just want the activity name for the list
                        if let Some(act_name) = a["activity"].as_str() {
                            activities_list.push(act_name.to_string());
                        }
                    }
                }
            }

            // Mood
            if let Some(m) = &clip.mood {
                *mood_counts.entry(m.to_string()).or_insert(0) += 1;
            }

            // Unusual
            if clip.is_unusual {
                // We don't have separate 'unusual_details' field, relying on description or just flagging it.
                // Maybe we can check if any activity is unusual if we stored it?
                // For now, just log the clip description
                let details = clip
                    .description
                    .clone()
                    .unwrap_or_else(|| "Unspecified".to_string());
                unusual_events.push(format!(
                    "Clip {} ({}): {}",
                    clip.id, clip.created_at, details
                ));
            }
        }

        if summaries.is_empty() && activities_list.is_empty() {
            continue;
        }

        // Create Final Digest Text
        let dominant_mood = mood_counts
            .iter()
            .max_by_key(|a| a.1)
            .map(|(k, _)| k.clone())
            .unwrap_or("Unknown".to_string());

        let final_summary = format!(
            "Daily Summary for {}\n\nHighlights:\n{}\n\nActivities:\n{}\n\nMood: {}\n\nUnusual Events:\n{}",
            date,
            summaries,
            activities_list.join(", "),
            dominant_mood,
            if unusual_events.is_empty() { "None".to_string() } else { unusual_events.join("\n") }
        );

        // Upsert DailyDigest
        // Check if exists
        let existing = DailyDigest::find()
            .filter(daily_digest::Column::PetId.eq(pet_id))
            .filter(daily_digest::Column::Date.eq(date))
            .one(&db)
            .await
            .unwrap_or(None);

        // Create JSON payloads
        let moods: Vec<String> = mood_counts.keys().cloned().collect();
        let moods_json = serde_json::to_value(moods).unwrap_or(serde_json::json!([]));
        let activities_json =
            serde_json::to_value(activities_list.clone()).unwrap_or(serde_json::json!([]));

        // Convert unusual_events strings back to detailed objects if possible, or just list
        // Since we only collected strings in previous loop, let's fix the loop to collect objects
        // We will do it properly by maintaining 'unusual_events_list'
        let mut unusual_events_objects = Vec::new();
        for clip in &clips {
            if clip.is_unusual {
                unusual_events_objects.push(serde_json::json!({
                    "video_id": clip.id,
                    "description": clip.description.clone().unwrap_or("Unusual activity".to_string()),
                    "timestamp": clip.created_at.to_rfc3339()
                }));
            }
        }
        let unusual_events_json =
            serde_json::to_value(unusual_events_objects).unwrap_or(serde_json::json!([]));

        if let Some(digest) = existing {
            let mut active: daily_digest::ActiveModel = digest.into();
            active.summary = Set(final_summary);
            active.moods = Set(Some(moods_json));
            active.activities = Set(Some(activities_json));
            active.unusual_events = Set(Some(unusual_events_json));
            active.total_videos = Set(clips.len() as i32);
            active.updated_at =
                Set(Utc::now().with_timezone(&chrono::FixedOffset::east_opt(0).unwrap()));
            let _ = active.update(&db).await;
        } else {
            let active = daily_digest::ActiveModel {
                id: Set(Uuid::new_v4()),
                pet_id: Set(pet_id),
                date: Set(date),
                summary: Set(final_summary),
                moods: Set(Some(moods_json)),
                activities: Set(Some(activities_json)),
                unusual_events: Set(Some(unusual_events_json)),
                total_videos: Set(clips.len() as i32),
                created_at: Set(
                    Utc::now().with_timezone(&chrono::FixedOffset::east_opt(0).unwrap())
                ),
                updated_at: Set(
                    Utc::now().with_timezone(&chrono::FixedOffset::east_opt(0).unwrap())
                ),
            };
            let _ = active.insert(&db).await;
        }

        generated_count += 1;
    }

    Ok(Json(json!({
        "message": "Daily digests generated",
        "count": generated_count,
        "date": date
    })))
}

#[derive(Deserialize)]
pub struct DigestPaginationParams {
    #[serde(default = "default_digest_page")]
    pub page: u64,
    #[serde(default = "default_digest_page_size")]
    pub page_size: u64,
}

fn default_digest_page() -> u64 {
    1
}
fn default_digest_page_size() -> u64 {
    10
}

#[derive(Serialize)]
pub struct DigestResponse {
    pub id: Uuid,
    pub pet_id: i32,
    pub date: chrono::NaiveDate,
    pub summary: String,
    pub moods: Option<serde_json::Value>,
    pub activities: Option<serde_json::Value>,
    pub unusual_events: Option<serde_json::Value>,
    pub total_videos: i32,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct DigestListResponse {
    pub digests: Vec<DigestResponse>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
}

// GET /pets/:id/digests - List daily digests for a pet
pub async fn list_pet_digests(
    Extension(db): Extension<DatabaseConnection>,
    Extension(user_id): Extension<i32>,
    Path(pet_id): Path<i32>,
    Query(params): Query<DigestPaginationParams>,
) -> impl IntoResponse {
    // Verify pet belongs to user
    let _pet = match pet::Entity::find_by_id(pet_id).one(&db).await {
        Ok(Some(p)) if p.user_id == user_id => p,
        Ok(Some(_)) => return (StatusCode::FORBIDDEN, "Not your pet").into_response(),
        Ok(None) => return (StatusCode::NOT_FOUND, "Pet not found").into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch pet: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };

    // Build query
    let query = DailyDigest::find()
        .filter(daily_digest::Column::PetId.eq(pet_id))
        .order_by_desc(daily_digest::Column::Date);

    // Get total count
    let total = match query.clone().count(&db).await {
        Ok(count) => count,
        Err(e) => {
            tracing::error!("Failed to count digests: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to count digests").into_response();
        }
    };

    // Fetch paginated results
    let paginator = query.paginate(&db, params.page_size);
    let digests_result = paginator.fetch_page(params.page - 1).await;

    match digests_result {
        Ok(digests) => {
            let response: Vec<DigestResponse> = digests
                .into_iter()
                .map(|digest| DigestResponse {
                    id: digest.id,
                    pet_id: digest.pet_id,
                    date: digest.date,
                    summary: digest.summary,
                    moods: digest.moods,
                    activities: digest.activities,
                    unusual_events: digest.unusual_events,
                    total_videos: digest.total_videos,
                    created_at: digest.created_at.to_rfc3339(),
                })
                .collect();

            (
                StatusCode::OK,
                Json(DigestListResponse {
                    digests: response,
                    total,
                    page: params.page,
                    page_size: params.page_size,
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to fetch digests: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch digests").into_response()
        }
    }
}
