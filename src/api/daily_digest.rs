use crate::entities::{daily_digest, pet_video, DailyDigest, PetVideo};
use axum::{
    extract::{Extension, Multipart, Path},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use chrono::Utc;
use google_cloud_storage::client::Client as GcsClient;
use google_cloud_storage::http::objects::upload::{UploadObjectRequest, UploadType};
use redis::AsyncCommands;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
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
                file_path: Set(gcs_path),
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
            let payload = serde_json::json!({ "video_id": file_uuid }).to_string();
            let _: () = conn.rpush("video_queue", payload).await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Redis Push Error: {}", e),
                )
            })?;

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

        for clip in clips {
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
            .into_iter()
            .max_by_key(|a| a.1)
            .map(|(k, _)| k)
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

        if let Some(digest) = existing {
            let mut active: daily_digest::ActiveModel = digest.into();
            active.summary = Set(final_summary);
            active.updated_at =
                Set(Utc::now().with_timezone(&chrono::FixedOffset::east_opt(0).unwrap()));
            let _ = active.update(&db).await;
        } else {
            let active = daily_digest::ActiveModel {
                id: Set(Uuid::new_v4()),
                pet_id: Set(pet_id),
                date: Set(date),
                summary: Set(final_summary),
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
