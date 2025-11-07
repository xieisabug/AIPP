pub mod prelude {
    // LLM entities live in db::llm_db to keep a single source of truth
    pub use crate::db::llm_db::llm_model::Entity as LlmModel;
    pub use crate::db::llm_db::llm_provider::Entity as LlmProvider;
    pub use crate::db::llm_db::llm_provider_config::Entity as LlmProviderConfig;

    // Re-export entities defined inside db modules to avoid duplication
    pub use crate::db::conversation_db::conversation::Entity as Conversation;
    pub use crate::db::conversation_db::message::Entity as Message;
    pub use crate::db::conversation_db::message_attachment::Entity as MessageAttachment;

    pub use crate::db::system_db::feature_config::Entity as FeatureConfig;
    pub use crate::db::system_db::system_config::Entity as SystemConfig;

    pub use crate::db::mcp_db::mcp_server::Entity as McpServer;
    pub use crate::db::mcp_db::mcp_server_prompt::Entity as McpServerPrompt;
    pub use crate::db::mcp_db::mcp_server_resource::Entity as McpServerResource;
    pub use crate::db::mcp_db::mcp_server_tool::Entity as McpServerTool;
    pub use crate::db::mcp_db::mcp_tool_call::Entity as McpToolCall;

    pub use crate::db::assistant_db::assistant::Entity as Assistant;
    pub use crate::db::assistant_db::assistant_mcp_config::Entity as AssistantMcpConfig;
    pub use crate::db::assistant_db::assistant_mcp_tool_config::Entity as AssistantMcpToolConfig;
    pub use crate::db::assistant_db::assistant_model::Entity as AssistantModel;
    pub use crate::db::assistant_db::assistant_model_config::Entity as AssistantModelConfig;
    pub use crate::db::assistant_db::assistant_prompt::Entity as AssistantPrompt;
    pub use crate::db::assistant_db::assistant_prompt_param::Entity as AssistantPromptParam;

    pub use crate::db::plugin_db::plugin_configurations::Entity as PluginConfigurations;
    pub use crate::db::plugin_db::plugin_data::Entity as PluginData;
    pub use crate::db::plugin_db::plugin_status::Entity as PluginStatus;
    pub use crate::db::plugin_db::plugins::Entity as Plugins;

    pub use crate::db::sub_task_db::sub_task_definition::Entity as SubTaskDefinition;
    pub use crate::db::sub_task_db::sub_task_execution::Entity as SubTaskExecution;

    // Artifacts
    pub use crate::db::artifacts_db::artifacts_collection::Entity as ArtifactsCollection;
}
