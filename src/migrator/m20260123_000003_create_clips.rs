use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Clip::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Clip::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Clip::VideoId).uuid().not_null())
                    .col(ColumnDef::new(Clip::StartTime).string().not_null())
                    .col(ColumnDef::new(Clip::EndTime).string().not_null())
                    .col(ColumnDef::new(Clip::Activity).string().not_null())
                    .col(ColumnDef::new(Clip::Mood).string().not_null())
                    .col(ColumnDef::new(Clip::Description).text().not_null())
                    .col(
                        ColumnDef::new(Clip::CreatedAt)
                            .timestamp_with_time_zone()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_clip_video_id")
                            .from(Clip::Table, Clip::VideoId)
                            .to(PetVideo::Table, PetVideo::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Clip::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum Clip {
    #[iden = "clips"]
    Table,
    Id,
    VideoId,
    StartTime,
    EndTime,
    Activity,
    Mood,
    Description,
    CreatedAt,
}

#[derive(Iden)]
enum PetVideo {
    Table,
    Id,
}
