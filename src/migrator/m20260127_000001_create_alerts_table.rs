use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Alerts::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Alerts::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Alerts::PetId).integer().not_null()) // Matches Pets::Id type
                    .col(ColumnDef::new(Alerts::AlertType).string().not_null())
                    .col(ColumnDef::new(Alerts::Severity).string().not_null())
                    .col(ColumnDef::new(Alerts::Message).text())
                    .col(ColumnDef::new(Alerts::Payload).json().not_null())
                    .col(ColumnDef::new(Alerts::InterventionAction).string())
                    .col(ColumnDef::new(Alerts::InterventionTime).date_time())
                    .col(ColumnDef::new(Alerts::Outcome).string())
                    .col(ColumnDef::new(Alerts::CreatedAt).date_time().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Alerts::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Alerts {
    Table,
    Id,
    PetId,
    AlertType,
    Severity,
    Message,
    Payload,
    InterventionAction,
    InterventionTime,
    Outcome,
    CreatedAt,
}
