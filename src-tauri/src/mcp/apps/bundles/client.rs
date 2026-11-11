//! MCP form elicitation uses the shared, conversation-scoped approval UI.
use crate::api::ai::acp::{AcpElicitationDecision, AcpElicitationRequestEvent, AcpElicitationState};
use rmcp::{ClientHandler, model::*, service::RequestContext, RoleClient};
use tauri::Manager;

pub struct PluginClient { pub app: tauri::AppHandle, pub conversation_id: Option<i64>, pub server_id: i64 }
struct PendingForm { app: tauri::AppHandle, id: String }
impl Drop for PendingForm {
    fn drop(&mut self) {
        let app = self.app.clone(); let id = self.id.clone();
        tokio::spawn(async move {
            let state = app.state::<AcpElicitationState>();
            if let Some(resolution) = state.resolve_request(&id,AcpElicitationDecision::Cancelled).await {
                crate::api::ai::acp::notify_cancelled_acp_elicitation_requests(&app,vec![(id,resolution)]).await;
            }
        });
    }
}
impl ClientHandler for PluginClient {
    fn get_info(&self) -> ClientInfo {
        let mut info = ClientInfo::default();
        if self.conversation_id.is_some() {
            info.capabilities.elicitation = Some(ElicitationCapability { form:Some(FormElicitationCapability::default()),url:None });
        }
        info
    }
    async fn create_elicitation(&self, request: CreateElicitationRequestParams, _context: RequestContext<RoleClient>) -> Result<CreateElicitationResult,ErrorData> {
        let error = |message: String| ErrorData::internal_error(message,None);
        let conversation = self.conversation_id.ok_or_else(||error("连接检测不能发起交互表单，请在对话调用工具时完成".into()))?;
        let CreateElicitationRequestParams::FormElicitationParams { message, requested_schema, .. } = request else {
            return Err(error("此客户端仅声明 form elicitation，服务端请求了未声明的 URL 模式".into()));
        };
        let schema = serde_json::to_value(requested_schema).map_err(|e|error(e.to_string()))?;
        let id = uuid::Uuid::new_v4().to_string();
        let (tx,rx) = tokio::sync::oneshot::channel();
        let state = self.app.state::<AcpElicitationState>();
        state.store_request_scoped(id.clone(),Some(conversation),tx,crate::api::ai::agent_driver::approval::AgentRequestScope::application(Some(conversation))).await.map_err(error)?;
        let _pending = PendingForm { app:self.app.clone(),id:id.clone() };
        let event = AcpElicitationRequestEvent { request_id:id,conversation_id:Some(conversation),agent_kind:"mcp".into(),message:format!("MCP 连接 {}：{message}",self.server_id),schema };
        state.attach_form(event.clone()).await.map_err(error)?;
        crate::api::operation_api::emit_permission_request_event(&self.app,crate::api::operation_api::ACP_ELICITATION_REQUEST_EVENT,Some(conversation),&event).map_err(error)?;
        let (action,content) = match rx.await {
            Ok(AcpElicitationDecision::Accepted(content)) => (ElicitationAction::Accept,Some(serde_json::to_value(content).map_err(|e|error(e.to_string()))?)),
            Ok(AcpElicitationDecision::Declined) => (ElicitationAction::Decline,None),
            _ => (ElicitationAction::Cancel,None),
        };
        Ok(CreateElicitationResult { action,content })
    }
}
