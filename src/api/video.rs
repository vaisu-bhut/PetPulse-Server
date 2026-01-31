use crate::entities::{pet, pet_video};
use axum::{
    body::Body,
    extract::{Extension, Path, Query},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use google_cloud_storage::client::Client as GcsClient;
use google_cloud_storage::http::objects::get::GetObjectRequest;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, PaginatorTrait};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_page")]
    pub page: u64,
    #[serde(default = "default_per_page")]
    pub per_page: u64,
}

fn default_page() -> u64 {
    1
}

fn default_per_page() -> u64 {
    10
}

#[derive(Debug, Serialize)]
pub struct VideoWithPet {
    #[serde(flatten)]
    pub video: pet_video::Model,
    pub pet: Option<pet::Model>,
}

#[derive(Debug, Serialize)]
pub struct VideoListResponse {
    pub videos: Vec<VideoWithPet>,
    pub total: u64,
    pub page: u64,
    pub per_page: u64,
    pub total_pages: u64,
}

pub async fn list_user_videos(
    Extension(db): Extension<DatabaseConnection>,
    Extension(user_id): Extension<i32>,
    Query(params): Query<PaginationParams>,
) -> Response {
    let user_pets = match pet::Entity::find()
        .filter(pet::Column::UserId.eq(user_id))
        .all(&db)
        .await
    {
        Ok(pets) => pets,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    let pet_ids: Vec<i32> = user_pets.iter().map(|p| p.id).collect();

    if pet_ids.is_empty() {
        return (
            StatusCode::OK,
            Json(VideoListResponse {
                videos: vec![],
                total: 0,
                page: params.page,
                per_page: params.per_page,
                total_pages: 0,
            }),
        )
            .into_response();
    }

    let paginator = pet_video::Entity::find()
        .filter(pet_video::Column::PetId.is_in(pet_ids.clone()))
        .filter(pet_video::Column::Status.eq("PROCESSED"))
        .order_by_desc(pet_video::Column::CreatedAt)
        .paginate(&db, params.per_page);

    let total = match paginator.num_pages().await {
        Ok(pages) => pages,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    let videos = match paginator.fetch_page(params.page - 1).await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    let pet_map: std::collections::HashMap<i32, pet::Model> =
        user_pets.into_iter().map(|p| (p.id, p)).collect();

    let videos_with_pets: Vec<VideoWithPet> = videos
        .into_iter()
        .map(|video| VideoWithPet {
            pet: pet_map.get(&video.pet_id).cloned(),
            video,
        })
        .collect();

    let total_items = match pet_video::Entity::find()
        .filter(pet_video::Column::PetId.is_in(pet_ids))
        .filter(pet_video::Column::Status.eq("PROCESSED"))
        .count(&db)
        .await
    {
        Ok(count) => count,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    (
        StatusCode::OK,
        Json(VideoListResponse {
            videos: videos_with_pets,
            total: total_items,
            page: params.page,
            per_page: params.per_page,
            total_pages: total,
        }),
    )
        .into_response()
}

pub async fn list_pet_videos(
    Extension(db): Extension<DatabaseConnection>,
    Path(pet_id): Path<i32>,
    Query(params): Query<PaginationParams>,
) -> Response {
    let paginator = pet_video::Entity::find()
        .filter(pet_video::Column::PetId.eq(pet_id))
        .filter(pet_video::Column::Status.eq("PROCESSED"))
        .order_by_desc(pet_video::Column::CreatedAt)
        .paginate(&db, params.per_page);

    let total = match paginator.num_pages().await {
        Ok(pages) => pages,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    let videos = match paginator.fetch_page(params.page - 1).await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    let pet = pet::Entity::find_by_id(pet_id).one(&db).await.ok().flatten();

    let videos_with_pets: Vec<VideoWithPet> = videos
        .into_iter()
        .map(|video| VideoWithPet {
            pet: pet.clone(),
            video,
        })
        .collect();

    let total_items = match pet_video::Entity::find()
        .filter(pet_video::Column::PetId.eq(pet_id))
        .filter(pet_video::Column::Status.eq("PROCESSED"))
        .count(&db)
        .await
    {
        Ok(count) => count,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    (
        StatusCode::OK,
        Json(VideoListResponse {
            videos: videos_with_pets,
            total: total_items,
            page: params.page,
            per_page: params.per_page,
            total_pages: total,
        }),
    )
        .into_response()
}

pub async fn serve_video(
    Extension(db): Extension<DatabaseConnection>,
    Extension(gcs_client): Extension<GcsClient>,
    Path(video_id): Path<String>,
) -> Response {
    // Parse video ID as UUID
    let video_uuid = match uuid::Uuid::parse_str(&video_id) {
        Ok(uuid) => uuid,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid video ID"})),
            )
                .into_response()
        }
    };

    // Get video from database
    let video = match pet_video::Entity::find_by_id(video_uuid).one(&db).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Video not found"})),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    // Extract GCS path (remove gs:// prefix and split bucket/object)
    let file_path = video.file_path.trim_start_matches("gs://");
    let parts: Vec<&str> = file_path.splitn(2, '/').collect();
    
    if parts.len() != 2 {
        tracing::error!("Invalid file path format: {}", video.file_path);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Invalid file path format"})),
        )
            .into_response();
    }

    let bucket = parts[0];
    let object_name = parts[1];

    tracing::info!("Fetching video from GCS: bucket={}, object={}", bucket, object_name);

    // Fetch video from GCS
    let request = GetObjectRequest {
        bucket: bucket.to_string(),
        object: object_name.to_string(),
        ..Default::default()
    };

    match gcs_client.download_object(&request, &Default::default()).await {
        Ok(data) => {
            tracing::info!("Successfully fetched video, size: {} bytes", data.len());
            // Return video file with proper content type
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "video/mp4"),
                    (header::CACHE_CONTROL, "public, max-age=3600"),
                    (header::CONTENT_LENGTH, data.len().to_string().as_str()),
                ],
                Body::from(data),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to fetch video from GCS: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to fetch video"})),
            )
                .into_response()
        }
    }
}
