use axum::{
    extract::{Extension, Path, Json},
    response::{IntoResponse, Response},
    http::StatusCode,
};
use sea_orm::{DatabaseConnection, EntityTrait, ActiveModelTrait, Set, IntoActiveModel};
use serde_json::json;
use crate::entities::pet;

#[derive(serde::Deserialize)]
pub struct CreatePetRequest {
    name: String,
    age: i32,
    species: String,
    breed: String,
    bio: String,
}

pub async fn create_pet(
    Extension(db): Extension<DatabaseConnection>,
    Extension(user_id): Extension<i32>,
    Json(payload): Json<CreatePetRequest>,
) -> Response {
    let now = chrono::Utc::now().naive_utc();
    let new_pet = pet::ActiveModel {
        user_id: Set(user_id),
        name: Set(payload.name),
        age: Set(payload.age),
        species: Set(payload.species),
        breed: Set(payload.breed),
        bio: Set(payload.bio),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    match new_pet.insert(&db).await {
        Ok(pet) => (StatusCode::CREATED, Json(pet)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn get_pet(
    Extension(db): Extension<DatabaseConnection>,
    Path(pet_id): Path<i32>,
) -> Response {
    match pet::Entity::find_by_id(pet_id).one(&db).await {
        Ok(Some(p)) => (StatusCode::OK, Json(p)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "Pet not found"}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct UpdatePetRequest {
    name: Option<String>,
    age: Option<i32>,
    species: Option<String>,
    breed: Option<String>,
    bio: Option<String>,
}

pub async fn update_pet(
    Extension(db): Extension<DatabaseConnection>,
    Path(pet_id): Path<i32>,
    Json(payload): Json<UpdatePetRequest>,
) -> Response {
    let pet = match pet::Entity::find_by_id(pet_id).one(&db).await {
        Ok(Some(p)) => p,
        Ok(None) => return (StatusCode::NOT_FOUND, Json(json!({"error": "Pet not found"}))).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    };

    let mut active_pet = pet.into_active_model();
    if let Some(name) = payload.name { active_pet.name = Set(name); }
    if let Some(age) = payload.age { active_pet.age = Set(age); }
    if let Some(species) = payload.species { active_pet.species = Set(species); }
    if let Some(breed) = payload.breed { active_pet.breed = Set(breed); }
    if let Some(bio) = payload.bio { active_pet.bio = Set(bio); }
    active_pet.updated_at = Set(chrono::Utc::now().naive_utc());

    match active_pet.update(&db).await {
        Ok(p) => (StatusCode::OK, Json(p)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn delete_pet(
    Extension(db): Extension<DatabaseConnection>,
    Path(pet_id): Path<i32>,
) -> Response {
    match pet::Entity::delete_by_id(pet_id).exec(&db).await {
        Ok(res) if res.rows_affected == 0 => (StatusCode::NOT_FOUND, Json(json!({"error": "Pet not found"}))).into_response(),
        Ok(_) => (StatusCode::OK, Json(json!({"message": "Pet deleted"}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}
