use sea_orm::{DatabaseConnection, EntityTrait, PaginatorTrait};
use crate::entities::{user, pet, pet_video};

pub async fn init_metrics(db: &DatabaseConnection) {
    // Total Counts
    let user_count = user::Entity::find().count(db).await.unwrap_or(0);
    metrics::gauge!("petpulse_users_total").set(user_count as f64);

    let pet_count = pet::Entity::find().count(db).await.unwrap_or(0);
    metrics::gauge!("petpulse_pets_total").set(pet_count as f64);

    let video_count = pet_video::Entity::find().count(db).await.unwrap_or(0);
    metrics::gauge!("petpulse_videos_total").set(video_count as f64);

    // Detailed Metrics for "Top 5" lists
    // 1. User Pets Count: Group users and count their pets
    // Since SeaORM group_by might be verbose, we can iterate or use custom select.
    // Let's iterate users for simplicity as cardinality is low in this demo.
    // Ideally use a join query: SELECT u.name, COUNT(p.id) FROM...
    // But for "init", simple iteration is safe enough for demo scale.
    
    use sea_orm::{QuerySelect, ModelTrait, LoaderTrait, ColumnTrait, QueryFilter};
    
    let users = user::Entity::find().all(db).await.unwrap_or_default();
    // Load pets for all users? Or just count?
    // Let's use a bespoke query for efficiency if possible, or just loop. 
    // Looping 21 users is instant.
    for u in users {
        let count = pet::Entity::find()
            .filter(pet::Column::UserId.eq(u.id))
            .count(db)
            .await
            .unwrap_or(0);
        metrics::gauge!("petpulse_user_pets_total", "name" => u.name).set(count as f64);
    }

    // 2. Pet Videos Count
    let pets = pet::Entity::find().all(db).await.unwrap_or_default();
    for p in pets {
        let count = pet_video::Entity::find()
            .filter(pet_video::Column::PetId.eq(p.id))
            .count(db)
            .await
            .unwrap_or(0);
        metrics::gauge!("petpulse_pet_videos_total", "name" => p.name).set(count as f64);
    }

    tracing::info!(
        "Initialized metrics: Users={}, Pets={}, Videos={}",
        user_count, pet_count, video_count
    );
}

pub async fn increment_user_pets(db: &DatabaseConnection, user_id: i32) {
    if let Ok(Some(u)) = user::Entity::find_by_id(user_id).one(db).await {
        metrics::gauge!("petpulse_user_pets_total", "name" => u.name).increment(1.0);
    }
}

pub async fn increment_pet_videos(db: &DatabaseConnection, pet_id: i32) {
    if let Ok(Some(p)) = pet::Entity::find_by_id(pet_id).one(db).await {
        metrics::gauge!("petpulse_pet_videos_total", "name" => p.name).increment(1.0);
    }
}

pub fn increment_critical_alerts(pet_id: i32) {
    metrics::counter!("petpulse_critical_alerts_total", "pet_id" => pet_id.to_string()).increment(1);
}

pub fn increment_notifications_sent(channel: &str) {
    metrics::counter!("petpulse_notifications_sent_total", "channel" => channel.to_string()).increment(1);
}

pub fn increment_notifications_failed(channel: &str) {
    metrics::counter!("petpulse_notifications_failed_total", "channel" => channel.to_string()).increment(1);
}

pub fn record_acknowledgment_time(seconds: f64) {
    metrics::histogram!("petpulse_alert_acknowledgment_duration_seconds").record(seconds);
}
