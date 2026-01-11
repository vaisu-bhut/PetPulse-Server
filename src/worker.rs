use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set, QueryFilter, ColumnTrait};
use redis::AsyncCommands;
use std::sync::Arc;
use tokio::sync::Semaphore;
use crate::entities::{pet_video, daily_digest, PetVideo, DailyDigest}; // Check exports
use crate::gemini::GeminiClient;
use chrono::Utc;
use uuid::Uuid;
use serde_json::Value;

pub async fn start_workers(redis_client: redis::Client, db: DatabaseConnection, concurrency: usize) {
    let db = Arc::new(db);
    let redis_client = Arc::new(redis_client);
    // Shared Gemini Client
    let gemini_client = Arc::new(GeminiClient::new());

    // We can just spawn 'concurrency' number of long-running loops.
    // Or a single loop that spawns tasks up to permit.
    // Given the requirement "max 3 concurrent LLM calls", 3 separate consumers is easiest.
    
    for i in 0..concurrency {
        let db = db.clone();
        let redis_client = redis_client.clone();
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
                
                // BLPOP - Block indefinitely (or 0 timeout)
                // BLPOP returns (key, value)
                let result: redis::RedisResult<(String, String)> = conn.blpop("video_queue", 0.0).await;
                
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
                        
                        process_video(video_id, &db, &gemini, &mut conn).await;
                    },
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
    redis_conn: &mut redis::aio::MultiplexedConnection
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

    // 3. Analyze
    match gemini.analyze_video(&video.file_path).await {
        Ok(analysis_result) => {
            tracing::info!("Analysis successful for {}", video_id);
            // Update Status COMPLETED
            let mut active: pet_video::ActiveModel = video.clone().into();
            active.status = Set("COMPLETED".to_string());
            active.analysis_result = Set(Some(analysis_result.clone()));
            let _ = active.update(db).await;

            // Aggregation (Simple: just append or upsert DailyDigest)
            // Validating existence of daily digest for this pet and date.
            let today = Utc::now().date_naive(); // Or use video creation date? Let's use today.
            
            let digest_opt = DailyDigest::find()
                .filter(daily_digest::Column::PetId.eq(video.pet_id))
                .filter(daily_digest::Column::Date.eq(today))
                .one(db)
                .await
                .unwrap_or(None);

            let analysis_text = analysis_result["candidates"][0]["content"]["parts"][0]["text"]
                .as_str()
                .unwrap_or("No analysis text found")
                .to_string();

            if let Some(digest) = digest_opt {
                // Update
                let new_summary = format!("{}\n\nNew Video Analysis:\n{}", digest.summary, analysis_text);
                let mut active_digest: daily_digest::ActiveModel = digest.into();
                active_digest.summary = Set(new_summary);
                let _ = active_digest.update(db).await;
            } else {
                // Create
                let new_summary = format!("Daily Digest for {}\n\nVideo Analysis:\n{}", today, analysis_text);
                let active_digest = daily_digest::ActiveModel {
                    id: Set(Uuid::new_v4()),
                    pet_id: Set(video.pet_id),
                    date: Set(today),
                    summary: Set(new_summary),
                    created_at: Set(Utc::now().into()),
                    updated_at: Set(Utc::now().into()),
                    ..Default::default()
                };
                let _ = active_digest.insert(db).await;
            }

        },
        Err(e) => {
            tracing::error!("Analysis failed for {}: {}", video_id, e);
            if retry_count < 2 {
                // Retry
                let mut active: pet_video::ActiveModel = video.clone().into();
                active.retry_count = Set(retry_count + 1);
                active.status = Set("Retrying".to_string());
                let _ = active.update(db).await;
                
                // Push back to Redis (Tail, or Head if we want immediate retry? User said FIFO so likely Tail)
                // But typically retries can go back to queue.
                let payload = serde_json::json!({ "video_id": video_id }).to_string();
                let _ : () = redis_conn.rpush("video_queue", payload).await.unwrap_or(());
            } else {
                // Fail
                let mut active: pet_video::ActiveModel = video.clone().into();
                active.status = Set("FAILED".to_string());
                let _ = active.update(db).await;
            }
        }
    }
}
