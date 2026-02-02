use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(QuickActions::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(QuickActions::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(QuickActions::AlertId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(QuickActions::EmergencyContactId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(QuickActions::ActionType)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(QuickActions::Message)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(QuickActions::VideoClips).json())
                    .col(
                        ColumnDef::new(QuickActions::Status)
                            .string()
                            .default("pending")
                            .not_null(),
                    )
                    .col(ColumnDef::new(QuickActions::SentAt).date_time())
                    .col(ColumnDef::new(QuickActions::AcknowledgedAt).date_time())
                    .col(ColumnDef::new(QuickActions::ErrorMessage).text())
                    .col(
                        ColumnDef::new(QuickActions::CreatedAt)
                            .date_time()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_quick_actions_alert")
                            .from(QuickActions::Table, QuickActions::AlertId)
                            .to(Alerts::Table, Alerts::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_quick_actions_contact")
                            .from(QuickActions::Table, QuickActions::EmergencyContactId)
                            .to(EmergencyContacts::Table, EmergencyContacts::Id)
                            .on_delete(ForeignKeyAction::Restrict)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_quick_actions_alert_id")
                    .table(QuickActions::Table)
                    .col(QuickActions::AlertId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_quick_actions_status")
                    .table(QuickActions::Table)
                    .col(QuickActions::Status)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(QuickActions::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum QuickActions {
    Table,
    Id,
    AlertId,
    EmergencyContactId,
    ActionType,
    Message,
    VideoClips,
    Status,
    SentAt,
    AcknowledgedAt,
    ErrorMessage,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Alerts {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum EmergencyContacts {
    Table,
    Id,
}
