use axum::{
    extract::{Multipart, Path, Extension, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};
use serde_json::json;
use std::path::PathBuf;
use tokio::fs;
use uuid::Uuid;
use chrono::Utc;
use redis::AsyncCommands;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::entities::{pet_video, PetVideo};

// We wrap the Redis connection in a Mutex for sharing (or use a Pool)
// For simplicity in this step, let's assume we pass a redis::Client or MultiplexedConnection
// Ideally, use a pool (deadpool-redis) or just clone the client if it supports it.
// Redis Client is cheap to clone, but MultiplexedConnection needs care.
// Let's assume we store `redis::Client` in Extension for now, and get a connection on demand.

pub async fn upload_video(
    Path(pet_id): Path<i32>,
    Extension(db): Extension<DatabaseConnection>,
    Extension(redis_client): Extension<redis::Client>, 
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    
    // 1. Parse Multipart to get file
    // We expect a field named "file" (or similar)
    // and maybe other metadata.
    
    let mut file_path_saved = String::new();
    let mut _file_name_original = String::new();

    while let Some(field) = multipart.next_field().await.map_err(|e: axum::extract::multipart::MultipartError| (StatusCode::BAD_REQUEST, e.to_string()))? {
        let name = field.name().unwrap_or("").to_string();
        
        if name == "video" {
            let file_name = field.file_name().unwrap_or("video.mp4").to_string();
            _file_name_original = file_name.clone();
            
            let data = field.bytes().await.map_err(|e: axum::extract::multipart::MultipartError| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            
            // Validate size (simple check, ideally stream it)
            if data.len() > 500 * 1024 * 1024 { // 500MB
                 return Err((StatusCode::PAYLOAD_TOO_LARGE, "File too large".to_string()));
            }

            // Define path: uploads/YYYY-MM-DD/pet_id/filename
            let date_str = Utc::now().format("%Y-%m-%d").to_string();
            let upload_dir = format!("uploads/{}/{}", date_str, pet_id);
            fs::create_dir_all(&upload_dir).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create dir: {}", e)))?;
            
            let file_uuid = Uuid::new_v4();
            // preserve extension
            let ext = std::path::Path::new(&file_name).extension().and_then(|s| s.to_str()).unwrap_or("mp4");
            let target_filename = format!("{}.{}", file_uuid, ext);
            let target_path = format!("{}/{}", upload_dir, target_filename);
            
            fs::write(&target_path, data).await.map_err(|e: std::io::Error| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to write file: {}", e)))?;
            
            file_path_saved = target_path;
            
            // We only process one file for now per request
            break;
        }
    }

    if file_path_saved.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "No video file found in field 'video'".to_string()));
    }

    // 2. Create PetVideo Entity
    let video_id = Uuid::new_v4();
    let now = Utc::now().with_timezone(&chrono::FixedOffset::east_opt(0).unwrap());

    let pet_video = pet_video::ActiveModel {
        id: Set(video_id),
        pet_id: Set(pet_id),
        file_path: Set(file_path_saved),
        status: Set("PENDING".to_string()),
        retry_count: Set(0),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    let _saved_video = pet_video.insert(&db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB Error: {}", e)))?;

    // 3. Push to Redis Queue
    // We need a connection
    let mut conn = redis_client.get_multiplexed_async_connection().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Redis Conn Error: {}", e)))?;
    
    let payload = serde_json::json!({ "video_id": video_id }).to_string();
    
    // RPUSH for FIFO (Task is pushed to tail)
    let _: () = conn.rpush("video_queue", payload).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Redis Push Error: {}", e)))?;

    Ok(Json(json!({
        "status": "queued",
        "video_id": video_id
    })))
}
