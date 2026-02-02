use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "quick_actions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub alert_id: Uuid,
    pub emergency_contact_id: i32,
    pub action_type: String,
    #[sea_orm(column_type = "Text")]
    pub message: String,
    pub video_clips: Option<Json>,
    pub status: String,
    pub sent_at: Option<DateTime>,
    pub acknowledged_at: Option<DateTime>,
    #[sea_orm(column_type = "Text", nullable)]
    pub error_message: Option<String>,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::alerts::Entity",
        from = "Column::AlertId",
        to = "super::alerts::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Alert,
    #[sea_orm(
        belongs_to = "super::emergency_contact::Entity",
        from = "Column::EmergencyContactId",
        to = "super::emergency_contact::Column::Id",
        on_update = "Cascade",
        on_delete = "Restrict"
    )]
    EmergencyContact,
}

impl Related<super::alerts::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Alert.def()
    }
}

impl Related<super::emergency_contact::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EmergencyContact.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
