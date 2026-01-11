use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "pet_video")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[serde(skip_deserializing)]
    pub id: Uuid,
    pub pet_id: i32,
    pub file_path: String,
    pub status: String,
    // Using JsonBinary for flexible storage of analysis results
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub analysis_result: Option<serde_json::Value>,
    pub retry_count: i32,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::pet::Entity",
        from = "Column::PetId",
        to = "super::pet::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Pet,
}

impl Related<super::pet::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Pet.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
