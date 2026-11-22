use super::{connection, delivery, error};
use crate::api::{ai::types::AiRequest, ai_api, butler_api};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

tokio::task_local! { pub static ACCEPTANCE: String; }
static DISPATCH: Mutex<()> = Mutex::const_new(());

pub(super) fn confirmation_matches(displayed: (Option<i64>, i64, Option<&str>), current: (i64, i64, &str)) -> bool {
    displayed == (Some(current.0), current.1, Some(current.2))
}

pub fn write_input(
    conn: &mut Connection,
    key: &str,
    conversation: i64,
    assistant: i64,
    text: &str,
    model_id: i64,
    model: &str,
) -> Result<i64> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM butler_proactive_acceptance a JOIN butler_proactive_item i USING(uuid)
        WHERE a.uuid=?1 AND a.conversation_id=?2 AND a.assistant_id=?3 AND a.status='pending' AND a.message_id IS NULL
        AND i.status='active' AND i.action_prompt=a.prompt AND a.prompt=?4)",
        params![key,conversation,assistant,crate::utils::turn_context::visible(text)],|r|r.get(0))?;
    ensure!(valid, "建议的接收会话已变化或消息已派发");
    let now = chrono::Utc::now();
    tx.execute("INSERT INTO message(conversation_id,message_type,content,created_time,llm_model_id,llm_model_name,token_count,input_token_count,
        output_token_count,metadata_json) VALUES(?1,'user',?2,?3,?4,?5,0,0,0,json_object('proactive_acceptance',?6))",
        params![conversation,text,now,model_id,model,key])?;
    let id = tx.last_insert_rowid();
    tx.execute("UPDATE butler_proactive_acceptance SET message_id=?2,status='sent',error=NULL WHERE uuid=?1",params![key,id])?;
    tx.execute("UPDATE butler_proactive_item SET status='done' WHERE uuid=?1", [key])?;
    tx.execute("UPDATE conversation SET updated_time=?2 WHERE id=?1", params![conversation, now])?;
    tx.execute("DELETE FROM conversation_summary WHERE conversation_id=?1", [conversation])?;
    tx.commit()?;
    Ok(id)
}

fn target(app: &AppHandle, owner: Option<i64>) -> Result<(i64, i64, String)> {
    if let Some(owner) = owner {
        if let Ok((id, name)) = delivery::contact(app, owner) {
            return Ok((id, owner, name));
        }
    }
    let id = butler_api::active_butler_main_conversation_id(app)
        .map_err(anyhow::Error::msg)?
        .context("总管家主会话不可用")?;
    let owner = connection(app)?.query_row(
        "SELECT assistant_id FROM conversation WHERE id=?1",
        [id],
        |r| r.get(0),
    )?;
    Ok((id, owner, "总管家".into()))
}

async fn dispatch(app: &AppHandle, key: &str) -> Result<()> {
    let conn = connection(app)?;
    let (conversation,owner,prompt,source):(i64,i64,String,Option<i64>)=conn.query_row("SELECT a.conversation_id,a.assistant_id,a.prompt,i.owner_assistant_id
        FROM butler_proactive_acceptance a JOIN butler_proactive_item i USING(uuid) WHERE a.uuid=?1 AND a.status='pending'",[key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    let (current, actor, _) = target(app, source)?;
    if current != conversation || actor != owner {
        conn.execute(
            "DELETE FROM butler_proactive_acceptance WHERE uuid=?1 AND message_id IS NULL",
            [key],
        )?;
        anyhow::bail!("目标助手或私聊已变化，请重新确认")
    }
    if app
        .state::<crate::ConversationActivityManager>()
        .get_runtime_state(conversation)
        .await
        .is_running
    {
        return Ok(());
    }
    let window =
        butler_api::resolve_or_create_butler_execution_window(app).map_err(anyhow::Error::msg)?;
    let request = AiRequest {
        conversation_id: conversation.to_string(),
        assistant_id: owner,
        prompt,
        model: None,
        override_model_id: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: Some(true),
        attachment_list: None,
        quote: None,
        butler_intent: None,
        butler_goal_revision: None,
    };
    let result = ACCEPTANCE
        .scope(
            key.to_owned(),
            ai_api::ask_ai(
                app.clone(),
                app.state(),
                app.state(),
                app.state(),
                app.state(),
                app.state(),
                window,
                request,
                None,
                None,
                None,
                None,
                None,
            ),
        )
        .await;
    let result = result.map_err(|e| anyhow::anyhow!("{e}")).and_then(|r| match r.error_message {
        Some(e) => Err(anyhow::anyhow!("{e}")),
        None => Ok(()),
    });
    if let Err(e) = &result {
        conn.execute("UPDATE butler_proactive_acceptance SET error=?2,status=CASE WHEN message_id IS NULL THEN 'failed' ELSE status END
        WHERE uuid=?1",params![key,format!("{e:#}")])?;
    }
    result
}

pub async fn recover(app: &AppHandle) -> Result<()> {
    let Ok(_guard) = DISPATCH.try_lock() else { return Ok(()) };
    let key:Option<String>=connection(app)?.query_row("SELECT uuid FROM butler_proactive_acceptance WHERE status='pending' ORDER BY accepted_at LIMIT 1",[],|r|r.get(0)).optional()?;
    if let Some(key) = key {
        if let Err(error) = dispatch(app, &key).await {
            connection(app)?.execute("UPDATE butler_proactive_acceptance SET status='failed',error=?2 WHERE uuid=?1 AND status='pending' AND message_id IS NULL",
                params![key,format!("{error:#}")])?;
            return Err(error);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn accept_butler_proactive_proposal(
    app: AppHandle,
    id: i64,
    revision: i64,
    confirmed_assistant: Option<i64>,
    confirmed_instruction: Option<String>,
) -> Result<serde_json::Value, String> {
    accept(&app, id, revision, confirmed_assistant, confirmed_instruction.as_deref()).await.map_err(error)
}

async fn accept(
    app: &AppHandle,
    id: i64,
    revision: i64,
    confirmed: Option<i64>,
    displayed: Option<&str>,
) -> Result<serde_json::Value> {
    let _guard = DISPATCH.lock().await;
    let mut conn = connection(app)?;
    let (key, owner, instruction, rev, status): (String, Option<i64>, String, i64, String) = conn
        .query_row(
        "SELECT uuid,owner_assistant_id,action_prompt,
        revision,status FROM butler_proactive_item WHERE id=?1 AND kind='proposal'",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    let receipt = |conn: &Connection| -> Result<Option<serde_json::Value>> {
        Ok(conn.query_row("SELECT a.conversation_id,a.message_id FROM butler_proactive_acceptance a JOIN message m
        ON m.id=a.message_id AND m.conversation_id=a.conversation_id AND m.message_type='user' WHERE a.uuid=?1",
        [&key],|r|Ok(serde_json::json!({"status":"sent","conversation":r.get::<_,i64>(0)?,"message":r.get::<_,i64>(1)?}))).optional()?)
    };
    if let Some(sent) = receipt(&conn)? {
        return Ok(sent);
    }
    if confirmed.is_none() {
        let pending=conn.query_row("SELECT conversation_id FROM butler_proactive_acceptance WHERE uuid=?1 AND status='pending'",
            [&key],|r|r.get::<_,i64>(0)).optional()?;
        if let Some(conversation) = pending {
            return Ok(serde_json::json!({"status":"pending","conversation":conversation}));
        }
    }
    ensure!(status == "active", "此建议已关闭");
    let (conversation, assistant, name) = target(app, owner)?;
    let preview = serde_json::json!({"status":"confirm","assistant":assistant,"name":name,"instruction":instruction,
        "revision":rev,"redirected":owner.is_some_and(|o|o!=assistant)});
    if !confirmation_matches((confirmed,revision,displayed),(assistant,rev,&instruction)) {
        return Ok(preview);
    }
    ensure!(super::config(app)?.enabled, "请在负责主动整理的设备上确认建议");
    crate::sync::proactive_owner(app, true).await.map_err(anyhow::Error::msg)?;
    {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM butler_proactive_item WHERE id=?1 AND revision=?2 AND status='active' AND action_prompt=?3)",
        params![id,rev,instruction],|r|r.get(0))?;
        ensure!(valid, "指令已变化，请重新打开卡片确认");
        tx.execute("INSERT INTO butler_proactive_acceptance(uuid,conversation_id,assistant_id,prompt,status,accepted_at)
        VALUES(?1,?2,?3,?4,'pending',unixepoch()) ON CONFLICT(uuid) DO UPDATE SET status='pending',error=NULL,conversation_id=excluded.conversation_id,
        assistant_id=excluded.assistant_id,prompt=excluded.prompt WHERE message_id IS NULL",params![key,conversation,assistant,instruction])?;
        tx.commit()?;
    }
    dispatch(app, &key).await?;
    Ok(receipt(&conn)?
        .unwrap_or_else(|| serde_json::json!({"status":"pending","conversation":conversation})))
}
