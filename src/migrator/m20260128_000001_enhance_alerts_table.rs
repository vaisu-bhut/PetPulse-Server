use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Add new columns for critical alert system
        manager
            .alter_table(
                Table::alter()
                    .table(Alerts::Table)
                    // Severity level classification
                    .add_column(
                        ColumnDef::new(Alerts::SeverityLevel)
                            .string()
                            .default("low")
                            .not_null()
                    )
                    // Critical condition indicators (JSON array)
                    .add_column(
                        ColumnDef::new(Alerts::CriticalIndicators)
                            .json()
                    )
                    // Recommended actions for user (JSON array)
                    .add_column(
                        ColumnDef::new(Alerts::RecommendedActions)
                            .json()
                    )
                    // User notification tracking
                    .add_column(
                        ColumnDef::new(Alerts::UserNotifiedAt)
                            .date_time()
                    )
                    .add_column(
                        ColumnDef::new(Alerts::UserAcknowledgedAt)
                            .date_time()
                    )
                    .add_column(
                        ColumnDef::new(Alerts::UserResponse)
                            .text()
                    )
                    .add_column(
                        ColumnDef::new(Alerts::NotificationSent)
                            .boolean()
                            .default(false)
                            .not_null()
                    )
                    // Notification channels used (JSON object)
                    .add_column(
                        ColumnDef::new(Alerts::NotificationChannels)
                            .json()
                    )
                    .to_owned(),
            )
            .await?;

        // Create indexes for efficient queries
        manager
            .create_index(
                Index::create()
                    .name("idx_alerts_severity_level")
                    .table(Alerts::Table)
                    .col(Alerts::SeverityLevel)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_alerts_unacknowledged")
                    .table(Alerts::Table)
                    .col(Alerts::UserNotifiedAt)
                    // PostgreSQL partial index: only index rows where user_acknowledged_at IS NULL
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop indexes first
        manager
            .drop_index(
                Index::drop()
                    .name("idx_alerts_unacknowledged")
                    .table(Alerts::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_alerts_severity_level")
                    .table(Alerts::Table)
                    .to_owned(),
            )
            .await?;

        // Drop columns
        manager
            .alter_table(
                Table::alter()
                    .table(Alerts::Table)
                    .drop_column(Alerts::SeverityLevel)
                    .drop_column(Alerts::CriticalIndicators)
                    .drop_column(Alerts::RecommendedActions)
                    .drop_column(Alerts::UserNotifiedAt)
                    .drop_column(Alerts::UserAcknowledgedAt)
                    .drop_column(Alerts::UserResponse)
                    .drop_column(Alerts::NotificationSent)
                    .drop_column(Alerts::NotificationChannels)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Alerts {
    Table,
    SeverityLevel,
    CriticalIndicators,
    RecommendedActions,
    UserNotifiedAt,
    UserAcknowledgedAt,
    UserResponse,
    NotificationSent,
    NotificationChannels,
}
