use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "llm_provider")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub name: String,
    pub api_type: String,
    pub description: Option<String>,
    #[sea_orm(default_value = false)]
    pub is_official: bool,
    #[sea_orm(default_value = false)]
    pub is_enabled: bool,
    #[sea_orm(column_type = "Timestamp", default_expr = "CURRENT_TIMESTAMP")]
    pub created_time: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {}
    }
}

impl ActiveModelBehavior for ActiveModel {}
