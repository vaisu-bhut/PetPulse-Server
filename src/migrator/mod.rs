use sea_orm_migration::prelude::*;

mod m20260101_000001_create_table;
mod m20260110_000002_create_daily_digest;
mod m20260123_000003_create_clips;
mod m20260123_000004_alter_pet_videos;
mod m20260123_000005_alter_daily_digest;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260101_000001_create_table::Migration),
            Box::new(m20260110_000002_create_daily_digest::Migration),
            Box::new(m20260123_000003_create_clips::Migration),
            Box::new(m20260123_000004_alter_pet_videos::Migration),
            Box::new(m20260123_000005_alter_daily_digest::Migration),
        ]
    }
}
