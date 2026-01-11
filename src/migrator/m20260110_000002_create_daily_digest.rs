use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create DailyDigest Table
        manager
            .create_table(
                Table::create()
                    .table(DailyDigest::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DailyDigest::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(DailyDigest::PetId).integer().not_null())
                    .col(ColumnDef::new(DailyDigest::Date).date().not_null())
                    .col(ColumnDef::new(DailyDigest::Summary).text().not_null())
                    .col(
                        ColumnDef::new(DailyDigest::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(DailyDigest::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-daily_digest-pet_id")
                            .from(DailyDigest::Table, DailyDigest::PetId)
                            .to(Pet::Table, Pet::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create PetVideo Table
        manager
            .create_table(
                Table::create()
                    .table(PetVideo::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PetVideo::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(PetVideo::PetId).integer().not_null())
                    .col(ColumnDef::new(PetVideo::FilePath).string().not_null())
                    .col(
                        ColumnDef::new(PetVideo::Status)
                            .string()
                            .not_null()
                            .default("PENDING"),
                    )
                    .col(ColumnDef::new(PetVideo::AnalysisResult).json_binary().null())
                    .col(
                        ColumnDef::new(PetVideo::RetryCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(PetVideo::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(PetVideo::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-pet_video-pet_id")
                            .from(PetVideo::Table, PetVideo::PetId)
                            .to(Pet::Table, Pet::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PetVideo::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(DailyDigest::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
pub enum DailyDigest {
    Table,
    Id,
    PetId,
    Date,
    Summary,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
pub enum PetVideo {
    Table,
    Id,
    PetId,
    FilePath,
    Status,
    AnalysisResult,
    RetryCount,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
pub enum Pet {
    #[iden = "pets"]
    Table,
    Id,
}
