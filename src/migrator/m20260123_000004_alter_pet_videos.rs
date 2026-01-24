use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop 'clips' if it exists
        manager
            .drop_table(Table::drop().table(Clip::Table).if_exists().to_owned())
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(PetVideo::Table)
                    .drop_column(PetVideo::AnalysisResult)
                    .add_column(ColumnDef::new(PetVideo::Activities).json_binary().null())
                    .add_column(ColumnDef::new(PetVideo::Mood).string().null())
                    .add_column(ColumnDef::new(PetVideo::Description).text().null())
                    .add_column(ColumnDef::new(PetVideo::IsUnusual).boolean().default(false))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(PetVideo::Table)
                    .add_column(
                        ColumnDef::new(PetVideo::AnalysisResult)
                            .json_binary()
                            .null(),
                    )
                    .drop_column(PetVideo::Activities)
                    .drop_column(PetVideo::Mood)
                    .drop_column(PetVideo::Description)
                    .drop_column(PetVideo::IsUnusual)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum PetVideo {
    Table,
    AnalysisResult,
    Activities,
    Mood,
    Description,
    IsUnusual,
}

#[derive(Iden)]
enum Clip {
    #[iden = "clips"]
    Table,
}
