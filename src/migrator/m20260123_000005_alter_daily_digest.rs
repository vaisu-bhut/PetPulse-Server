use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(DailyDigest::Table)
                    .add_column(ColumnDef::new(DailyDigest::Moods).json_binary().null())
                    .add_column(ColumnDef::new(DailyDigest::Activities).json_binary().null())
                    .add_column(
                        ColumnDef::new(DailyDigest::UnusualEvents)
                            .json_binary()
                            .null(),
                    )
                    .add_column(
                        ColumnDef::new(DailyDigest::TotalVideos)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(DailyDigest::Table)
                    .drop_column(DailyDigest::Moods)
                    .drop_column(DailyDigest::Activities)
                    .drop_column(DailyDigest::UnusualEvents)
                    .drop_column(DailyDigest::TotalVideos)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum DailyDigest {
    Table,
    Moods,
    Activities,
    UnusualEvents,
    TotalVideos,
}
