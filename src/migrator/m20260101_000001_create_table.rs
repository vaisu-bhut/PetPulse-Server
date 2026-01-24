use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create Users Table
        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Users::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Users::Email)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(Users::PasswordHash).string().not_null())
                    .col(ColumnDef::new(Users::Name).string().not_null())
                    .col(ColumnDef::new(Users::CreatedAt).date_time().not_null())
                    .col(ColumnDef::new(Users::UpdatedAt).date_time().not_null())
                    .to_owned(),
            )
            .await?;

        // Create Pets Table
        manager
            .create_table(
                Table::create()
                    .table(Pets::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Pets::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Pets::UserId).integer().not_null())
                    .col(ColumnDef::new(Pets::Name).string().not_null())
                    .col(ColumnDef::new(Pets::Age).integer().not_null())
                    .col(ColumnDef::new(Pets::Species).string().not_null())
                    .col(ColumnDef::new(Pets::Breed).string().not_null())
                    .col(ColumnDef::new(Pets::Bio).text().not_null())
                    .col(ColumnDef::new(Pets::CreatedAt).date_time().not_null())
                    .col(ColumnDef::new(Pets::UpdatedAt).date_time().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-pet-user_id")
                            .from(Pets::Table, Pets::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Pets::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Users::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    Email,
    PasswordHash,
    Name,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Pets {
    Table,
    Id,
    UserId,
    Name,
    Age,
    Species,
    Breed,
    Bio,
    CreatedAt,
    UpdatedAt,
}
