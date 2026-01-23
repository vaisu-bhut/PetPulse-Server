use crate::entities::{pet_video, PetVideo};
use crate::gemini::GeminiClient;
use chrono::Utc;
use google_cloud_storage::client::Client as GcsClient;
use google_cloud_storage::http::objects::download::Range;
use google_cloud_storage::http::objects::get::GetObjectRequest;
use redis::AsyncCommands;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

pub async fn start_workers(
    redis_client: redis::Client,
    db: DatabaseConnection,
    concurrency: usize,
    gcs_client: GcsClient,
) {
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

                        process_video(video_id, &db, &gemini, &mut conn, &gcs_client).await;
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
) {
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
        return;
    }

    // 3. Download from GCS
    let gcs_path = video.file_path.clone();
    let temp_file_path = format!("/tmp/{}", video_id);

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
            return;
        }
    };

    if let Err(e) = tokio::fs::write(&temp_file_path, data).await {
        tracing::error!("Failed to write temp file: {}", e);
        return;
    }

    // 4. Analyze
    match gemini.analyze_video(&temp_file_path).await {
        Ok(analysis_result) => {
            tracing::info!("Analysis successful for {}", video_id);
            tracing::info!("Raw Analysis Result: {:?}", analysis_result);

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

            tracing::info!(
                "Updating video {} with: mood={:?}, unusual={:?}",
                video_id,
                active.mood,
                active.is_unusual
            );

            match active.update(db).await {
                Ok(v) => tracing::info!("Updated video successfully: {:?}", v),
                Err(e) => tracing::error!("Failed to update video {}: {}", video_id, e),
            }

            // Cleanup
            let _ = tokio::fs::remove_file(&temp_file_path).await;
        }
        Err(e) => {
            tracing::error!("Analysis failed for {}: {}", video_id, e);

            // Cleanup
            let _ = tokio::fs::remove_file(&temp_file_path).await;

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
}
