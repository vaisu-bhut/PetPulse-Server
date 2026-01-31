use sea_orm_migration::prelude::*;

mod m20260101_000001_create_table;
mod m20260110_000002_create_daily_digest;
mod m20260123_000003_create_clips;
mod m20260123_000004_alter_pet_videos;
mod m20260123_000005_alter_daily_digest;
mod m20260127_000001_create_alerts_table;
mod m20260128_000001_enhance_alerts_table;
mod m20260130_000001_create_emergency_contacts;
mod m20260130_000002_create_quick_actions;

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
            Box::new(m20260127_000001_create_alerts_table::Migration),
            Box::new(m20260128_000001_enhance_alerts_table::Migration),
            Box::new(m20260130_000001_create_emergency_contacts::Migration),
            Box::new(m20260130_000002_create_quick_actions::Migration),
        ]
    }
}
