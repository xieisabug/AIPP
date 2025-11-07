use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "llm_model")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub name: String,
    pub llm_provider_id: i64,
    pub code: String,
    pub description: Option<String>,
    #[sea_orm(default_value = false)]
    pub vision_support: bool,
    #[sea_orm(default_value = false)]
    pub audio_support: bool,
    #[sea_orm(default_value = false)]
    pub video_support: bool,
    #[sea_orm(column_type = "Timestamp", default_expr = "CURRENT_TIMESTAMP")]
    pub created_time: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(belongs_to = "super::llm_provider::Entity", from = "Column::LlmProviderId", to = "super::llm_provider::Column::Id")]
    LlmProvider,
}

impl ActiveModelBehavior for ActiveModel {}
