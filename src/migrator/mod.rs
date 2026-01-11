use sea_orm_migration::prelude::*;

mod m20260101_000001_create_table;
mod m20260110_000002_create_daily_digest;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260101_000001_create_table::Migration),
            Box::new(m20260110_000002_create_daily_digest::Migration),
        ]
    }
}
