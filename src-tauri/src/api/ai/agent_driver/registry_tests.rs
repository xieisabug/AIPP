use super::*;
use super::super::capabilities::CapabilitySupport;
use super::super::dispatch::{dispatch_prepared, response_message, TurnEffects, TurnInput};
use super::super::session::{AgentPromptContext, AgentSessionControl};
use crate::api::ai::events::ConversationEvent;
use crate::api::assistant_api::AssistantDetail;
use crate::db::assistant_db::{Assistant, AssistantModel};
use crate::db::conversation_db::{Message, MessageAttachment, MessageRepository, Repository};
use std::sync::Mutex;

#[allow(dead_code)]
#[path = "../../../db/tests/test_helpers.rs"]
mod database_fixture;

type Actions = Arc<Mutex<Vec<String>>>;
struct FourthDriver { actions: Actions, operations: AgentSessionCapabilities, send_prompt_support: CapabilitySupport }
impl FourthDriver {
    fn new(actions: Actions) -> Self {
        Self { actions, operations: AgentSessionCapabilities { structured_attachments: true, cancel_prompt: true, set_config_option: true }, send_prompt_support: CapabilitySupport::Supported }
    }
}
fn metadata(operations: AgentSessionCapabilities) -> AgentDriverMetadata {
    AgentDriverMetadata { id: "test_extension", display_name: "Fourth extension", message_model_name: "custom-model", operations }
}

#[async_trait::async_trait]
impl AgentDriver for FourthDriver {
    fn metadata(&self) -> AgentDriverMetadata { metadata(self.operations) }
    fn capabilities(&self) -> AgentCapabilities {
        let mut capabilities = AgentCapabilities::baseline(self.metadata().id, AgentCapabilityDeclaration::unknown(
            self.operations.cancel_prompt, self.operations.set_config_option, self.operations.structured_attachments,
        ));
        capabilities.operations.send_prompt.support = self.send_prompt_support;
        capabilities
    }
    fn matches_provider(&self, api_type: &str, _configs: &[LLMProviderConfig]) -> bool { api_type == "test_extension" }
    fn config_schema(&self) -> Vec<AgentConfigField> {
        vec![AgentConfigField { name: "mode".into(), required: true, value_type: AgentConfigValueType::Choice(vec!["safe".into(), "plan".into()]) }]
    }
    fn prepare(&self, _request: &AgentTurnRequest<'_>, instance: &AgentInstanceConfig, model_override: Option<&str>) -> Result<Box<dyn PreparedAgentSession>, AppError> {
        let mode = instance.configs.iter().find(|config| config.name == "mode").unwrap().value.clone();
        self.actions.lock().unwrap().push(format!("prepare:{}:{mode}:{}", instance.provider_id, model_override.unwrap_or("default")));
        Ok(Box::new(FourthPrepared { provider_id: instance.provider_id, mode, actions: self.actions.clone(), metadata: self.metadata() }))
    }
    async fn lookup(&self, _app: &tauri::AppHandle, _conversation_id: i64) -> Result<Option<AgentSessionLookup>, AppError> { Ok(None) }
}
struct FourthPrepared { provider_id: i64, mode: String, actions: Actions, metadata: AgentDriverMetadata }
#[async_trait::async_trait]
impl PreparedAgentSession for FourthPrepared {
    fn metadata(&self) -> AgentDriverMetadata { self.metadata }
    async fn finalize(&mut self, _app: &tauri::AppHandle, _conversation_id: i64, _payload: String) -> Result<(), AppError> { Ok(()) }
    async fn acquire(self: Box<Self>, _host: &dyn AgentSessionHost, conversation_id: i64) -> Result<AgentSessionHandle, AppError> {
        self.actions.lock().unwrap().push(format!("acquire:{}:{conversation_id}:{}", self.provider_id, self.mode));
        Ok(AgentSessionHandle::new(FourthControl { actions: self.actions, metadata: self.metadata }, format!("custom-run-{}", self.provider_id)))
    }
}
struct FourthControl { actions: Actions, metadata: AgentDriverMetadata }
#[async_trait::async_trait]
impl AgentSessionControl for FourthControl {
    fn metadata(&self) -> AgentDriverMetadata { self.metadata }
    fn send_prompt(&self, message_id: i64, prompt: String, attachments: Vec<MessageAttachment>, _context: &dyn AgentPromptContext) -> Result<(), AppError> {
        self.actions.lock().unwrap().push(format!("submit:{message_id}:{prompt}:{}", attachments.len())); Ok(())
    }
    async fn cancel_current_prompt(&self) -> Result<(), AppError> { self.actions.lock().unwrap().push("cancel".into()); Ok(()) }
    async fn set_config_option(&self, id: String, value: String) -> Result<(), AppError> { self.actions.lock().unwrap().push(format!("config:{id}:{value}")); Ok(()) }
}
struct NoDesktop;
impl AgentSessionHost for NoDesktop {
    fn app_handle(&self) -> Result<&tauri::AppHandle, AppError> { Err(AppError::ProviderError("test has no desktop".into())) }
}
impl AgentPromptContext for NoDesktop {
    fn window(&self) -> Result<tauri::Window, AppError> { Err(AppError::ProviderError("test has no window".into())) }
}
fn config(provider_id: i64, value: &str) -> LLMProviderConfig {
    LLMProviderConfig { id: 1, llm_provider_id: provider_id, name: "mode".into(), value: value.into(), append_location: String::new(), is_addition: false }
}
fn assistant() -> AssistantDetail {
    AssistantDetail {
        assistant: Assistant { id: 7, name: "test".into(), description: None, assistant_type: Some(4), is_addition: false, created_time: String::new() },
        model: vec![AssistantModel { id: 11, assistant_id: 7, provider_id: 51, model_code: "configured".into(), alias: String::new() }],
        model_configs: vec![], prompts: vec![], prompt_params: vec![], mcp_configs: vec![], mcp_tool_configs: vec![],
    }
}
fn setup() -> (AgentDriverRegistry, Actions) {
    let actions = Arc::new(Mutex::new(Vec::new()));
    let registry = AgentDriverRegistry::default();
    super::super::native::register_builtin_drivers(&registry).unwrap();
    registry.register(Arc::new(FourthDriver::new(actions.clone()))).unwrap();
    (registry, actions)
}

#[test]
fn test_registry_duplicate_and_unknown_driver_fail_without_replacing_registration() {
    let (registry, actions) = setup();
    assert!(registry.register(Arc::new(FourthDriver::new(actions))).err().unwrap().to_string().contains("already registered"));
    assert_eq!(registry.drivers().unwrap().len(), 4);
    assert!(registry.driver("missing").err().unwrap().to_string().contains("no fallback"));
    assert!(registry.resolve_provider(1, "unknown", vec![]).err().unwrap().to_string().contains("unsupported provider API type 'unknown'"));
}

#[test]
fn test_registry_duplicate_unknown_and_cross_owned_instances_fail() {
    let (registry, _) = setup();
    let instance = AgentInstanceConfig::new(51, "test_extension", vec![config(51, "safe")]).unwrap();
    registry.register_instance(instance.clone()).unwrap();
    assert!(registry.register_instance(instance).err().unwrap().to_string().contains("already registered"));
    assert!(registry.instance(999).err().unwrap().to_string().contains("unknown provider instance 999"));
    assert!(AgentInstanceConfig::new(0, "test_extension", vec![]).is_err());
    assert!(AgentInstanceConfig::new(51, "test_extension", vec![config(52, "safe")]).err().unwrap().to_string().contains("owned by provider 52"));
    assert!(AgentInstanceConfig::new(51, "test_extension", vec![config(51, "safe"), config(51, "plan")]).err().unwrap().to_string().contains("duplicate config"));
    assert!(registry.register_instance(AgentInstanceConfig::new(99, "missing", vec![]).unwrap()).is_err());
}

#[test]
fn test_registry_schema_rejects_missing_invalid_modes_and_keeps_previous_valid_snapshot() {
    let (registry, _) = setup();
    let valid = registry.resolve_provider(51, "test_extension", vec![config(51, "safe")]).unwrap();
    for configs in [vec![], vec![config(51, "")], vec![config(51, "bypass")]] {
        assert!(registry.resolve_provider(51, "test_extension", configs).is_err());
        assert_eq!(registry.instance(51).unwrap().config_fingerprint, valid.config_fingerprint);
    }
    let updated = registry.resolve_provider(51, "test_extension", vec![config(51, "plan")]).unwrap();
    assert_ne!(updated.config_fingerprint, valid.config_fingerprint);
}

#[tokio::test]
async fn test_registry_same_driver_multiple_instances_prepare_isolated_configs() {
    let (registry, actions) = setup();
    let first = registry.resolve_provider(51, "test_extension", vec![config(51, "safe")]).unwrap();
    let second = registry.resolve_provider(52, "test_extension", vec![config(52, "plan")]).unwrap();
    assert_ne!(first.config_fingerprint, second.config_fingerprint);
    let assistant = assistant(); let overrides = None; let features = HashMap::new();
    let request = AgentTurnRequest { source_message_id: None, assistant: &assistant, conversation_id: 42, prompt: "runtime".into(), display_prompt: "display".into(), override_model_id: None, overrides: &overrides, feature_configs: &features, messages: &[] };
    let first_handle = registry.prepare(&first, &request, Some("model-a")).unwrap().acquire(&NoDesktop, 42).await.unwrap();
    let second_handle = registry.prepare(&second, &request, Some("model-b")).unwrap().acquire(&NoDesktop, 43).await.unwrap();
    assert_eq!(first_handle.run_id(), "custom-run-51"); assert_eq!(second_handle.run_id(), "custom-run-52");
    assert_eq!(*actions.lock().unwrap(), ["prepare:51:safe:model-a", "acquire:51:42:safe", "prepare:52:plan:model-b", "acquire:52:43:plan"]);
    assert_eq!(registry.instance(51).unwrap().configs[0].value, "safe");
}

struct HeadlessEffects { repo: Mutex<MessageRepository>, handle: Mutex<Option<AgentSessionHandle>> }
#[async_trait::async_trait]
impl TurnEffects for HeadlessEffects {
    type Prepared = Box<dyn PreparedAgentSession>;
    type Handle = AgentSessionHandle;
    fn create_response(&self, conversation_id: i64, metadata: AgentDriverMetadata) -> Result<Message, AppError> { self.repo.lock().unwrap().create(&response_message(conversation_id, metadata)).map_err(AppError::from) }
    fn emit(&self, _conversation_id: i64, _event: ConversationEvent) {}
    async fn acquire(&self, prepared: Self::Prepared, conversation_id: i64) -> Result<Self::Handle, AppError> {
        let handle = prepared.acquire(&NoDesktop, conversation_id).await?;
        *self.handle.lock().unwrap() = Some(handle.clone()); Ok(handle)
    }
    fn submit(&self, handle: &Self::Handle, id: i64, prompt: String, attachments: Vec<MessageAttachment>) -> Result<(), AppError> {
        assert!(self.repo.lock().unwrap().read(id).unwrap().is_some());
        handle.send_prompt_with_context(id, prompt, attachments, &NoDesktop)
    }
    async fn fail(&self, _conversation_id: i64, _message_id: i64, _metadata: AgentDriverMetadata, detail: &str) { panic!("unexpected dispatch failure: {detail}"); }
}

/// A real fourth registration runs through production resolve/prepare/dispatch,
/// real in-memory persistence, acquire, prompt and control trait boundaries.
#[tokio::test]
async fn test_registry_fourth_driver_runs_production_dispatch_and_controls() {
    let (registry, actions) = setup();
    let assistant = assistant(); let overrides = None; let features = HashMap::new();
    let request = AgentTurnRequest { source_message_id: None, assistant: &assistant, conversation_id: 42, prompt: "runtime".into(), display_prompt: "display".into(), override_model_id: None, overrides: &overrides, feature_configs: &features, messages: &[] };
    let (instance, model) = super::super::turn::load_agent_instance(&registry, &assistant, Some("selected%%51"), |id| Ok(("test_extension".into(), vec![config(id, "plan")]))).unwrap();
    let prepared = registry.prepare(&instance, &request, model.as_deref()).unwrap();
    let metadata = prepared.metadata();
    let effects = HeadlessEffects { repo: Mutex::new(MessageRepository::new(database_fixture::create_test_db())), handle: Mutex::new(None) };
    let result = dispatch_prepared(&effects, prepared, metadata, TurnInput { conversation_id: 42, prompt: "custom prompt".into(), display_prompt: "display".into(), messages: &[] }).await.unwrap();
    assert_eq!(result.conversation_id, 42);
    let handle = effects.handle.lock().unwrap().clone().unwrap();
    handle.cancel_current_prompt().await.unwrap();
    handle.set_config_option("vendor/opaque".into(), " exact ".into()).await.unwrap();
    let actions = actions.lock().unwrap();
    assert_eq!(actions[0], "prepare:51:plan:selected"); assert_eq!(actions[1], "acquire:51:42:plan");
    assert!(actions[2].contains(":custom prompt:0")); assert_eq!(actions[3], "cancel"); assert_eq!(actions[4], "config:vendor/opaque: exact ");
    let message_id = actions[2].split(':').nth(1).unwrap().parse().unwrap();
    assert_eq!(effects.repo.lock().unwrap().read(message_id).unwrap().unwrap().llm_model_name.as_deref(), Some("custom-model"));
}

#[tokio::test]
async fn test_registry_unsupported_controls_never_call_registered_actor() {
    let actions = Arc::new(Mutex::new(Vec::new()));
    let metadata = metadata(AgentSessionCapabilities { structured_attachments: false, cancel_prompt: false, set_config_option: false });
    let handle = AgentSessionHandle::new(FourthControl { actions: actions.clone(), metadata }, "run".into());
    assert!(handle.cancel_current_prompt().await.err().unwrap().to_string().contains("test_extension"));
    assert!(handle.set_config_option("mode".into(), "plan".into()).await.is_err());
    assert!(actions.lock().unwrap().is_empty());
}

#[test]
fn test_registry_unsupported_send_prompt_rejects_before_driver_preparation() {
    let actions = Arc::new(Mutex::new(Vec::new()));
    let registry = AgentDriverRegistry::default();
    let mut driver = FourthDriver::new(actions.clone());
    driver.send_prompt_support = CapabilitySupport::Unsupported;
    registry.register(Arc::new(driver)).unwrap();
    let instance = registry.resolve_provider(51, "test_extension", vec![config(51, "safe")]).unwrap();
    let assistant = assistant(); let overrides = None; let features = HashMap::new();
    let request = AgentTurnRequest { source_message_id: None, assistant: &assistant, conversation_id: 42, prompt: "runtime".into(), display_prompt: "display".into(), override_model_id: None, overrides: &overrides, feature_configs: &features, messages: &[] };
    let error = registry.prepare(&instance, &request, None).err().unwrap().to_string();
    assert!(error.contains("test_extension")); assert!(error.contains("send_prompt")); assert!(error.contains("unsupported"));
    assert!(actions.lock().unwrap().is_empty(), "unsupported declaration must reject before prepare/acquire/submit");
}

#[test]
fn test_registry_unknown_send_prompt_defers_to_registered_driver() {
    let actions = Arc::new(Mutex::new(Vec::new()));
    let registry = AgentDriverRegistry::default();
    let mut driver = FourthDriver::new(actions.clone());
    driver.send_prompt_support = CapabilitySupport::Unknown;
    registry.register(Arc::new(driver)).unwrap();
    let instance = registry.resolve_provider(51, "test_extension", vec![config(51, "safe")]).unwrap();
    let assistant = assistant(); let overrides = None; let features = HashMap::new();
    let request = AgentTurnRequest { source_message_id: None, assistant: &assistant, conversation_id: 42, prompt: "runtime".into(), display_prompt: "display".into(), override_model_id: None, overrides: &overrides, feature_configs: &features, messages: &[] };
    assert!(registry.prepare(&instance, &request, None).is_ok());
    assert_eq!(*actions.lock().unwrap(), ["prepare:51:safe:default"]);
}
