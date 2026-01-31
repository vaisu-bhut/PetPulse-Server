use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(EmergencyContacts::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(EmergencyContacts::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(EmergencyContacts::UserId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(EmergencyContacts::ContactType)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(EmergencyContacts::Name)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(EmergencyContacts::Phone)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(EmergencyContacts::Email).string())
                    .col(ColumnDef::new(EmergencyContacts::Address).text())
                    .col(ColumnDef::new(EmergencyContacts::Notes).text())
                    .col(
                        ColumnDef::new(EmergencyContacts::Priority)
                            .integer()
                            .default(0)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(EmergencyContacts::IsActive)
                            .boolean()
                            .default(true)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(EmergencyContacts::CreatedAt)
                            .date_time()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(EmergencyContacts::UpdatedAt)
                            .date_time()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_emergency_contacts_user")
                            .from(EmergencyContacts::Table, EmergencyContacts::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_emergency_contacts_user_id")
                    .table(EmergencyContacts::Table)
                    .col(EmergencyContacts::UserId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_emergency_contacts_type")
                    .table(EmergencyContacts::Table)
                    .col(EmergencyContacts::ContactType)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(EmergencyContacts::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum EmergencyContacts {
    Table,
    Id,
    UserId,
    ContactType,
    Name,
    Phone,
    Email,
    Address,
    Notes,
    Priority,
    IsActive,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
