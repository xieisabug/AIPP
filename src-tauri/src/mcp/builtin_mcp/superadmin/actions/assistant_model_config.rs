//! Targeted parameter repair, with compare-and-swap and transactional undo.
use async_trait::async_trait;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use tauri::AppHandle;
use crate::db::assistant_db::AssistantDatabase;
use super::super::{registry::{ActionHandler, ActionRegistry}, types::*};

#[cfg(test)]
#[path = "assistant_model_config_tests.rs"]
mod tests;

fn snapshot(conn: &Connection, assistant: i64) -> Result<Value, String> {
    let mut stmt = conn.prepare("SELECT name,value,value_type,assistant_model_id FROM assistant_model_config WHERE assistant_id=?1 ORDER BY name").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([assistant], |r| Ok(json!({
        "name": r.get::<_, String>(0)?, "value": r.get::<_, Option<String>>(1)?,
        "value_type": r.get::<_, String>(2)?, "assistant_model_id": r.get::<_, i64>(3)?
    }))).map_err(|e| e.to_string())?;
    Ok(json!(rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?))
}

fn value_type(name: &str, value: &Value) -> Result<&'static str, String> {
    match name {
        "temperature" | "top_p" if value.as_f64().is_some_and(|v| v.is_finite() && v >= 0.0 && v <= if name == "top_p" { 1.0 } else { 2.0 }) => Ok("float"),
        "max_tokens" if value.as_u64().is_some_and(|v| v > 0 && v <= u32::MAX as u64) => Ok("integer"),
        "reasoning_effort" if value.as_str().and_then(genai::chat::ReasoningEffort::from_keyword).is_some() => Ok("string"),
        _ => Err(format!("不支持的参数或无效值：{name}；仅支持 temperature、top_p、max_tokens、reasoning_effort，删除请使用 remove")),
    }
}

fn patch(conn: &mut Connection, args: &Value, dry_run: bool) -> Result<Value, String> {
    let assistant = args["assistant_id"].as_i64().ok_or("需要 assistant_id")?;
    let model = args["assistant_model_id"].as_i64().ok_or("需要 assistant_model_id（助手模型绑定 ID）")?;
    let expected = args.get("expected_configs").filter(|v| v.is_array()).ok_or("需要 assistant.get 返回的 model_configs 作为 expected_configs")?;
    let set = args["set"].as_object().ok_or("set 必须是对象，可为空")?;
    let remove = args["remove"].as_array().ok_or("remove 必须是数组，可为空")?;
    if set.is_empty() && remove.is_empty() { return Err("参数补丁不能为空".into()); }
    for (name, value) in set { value_type(name, value)?; }
    for value in remove {
        let name = value.as_str().ok_or("remove 的元素必须为参数名")?;
        if !matches!(name, "temperature" | "top_p" | "max_tokens" | "reasoning_effort") || set.contains_key(name) {
            return Err(format!("删除参数不支持或与 set 冲突：{name}"));
        }
    }
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
    let owned: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM assistant_model WHERE id=?1 AND assistant_id=?2)", params![model, assistant], |r| r.get(0)).map_err(|e| e.to_string())?;
    if !owned { return Err("模型绑定不属于指定助手，请重新读取 assistant.get".into()); }
    let before = snapshot(&tx, assistant)?;
    if &before != expected { return Err("助手参数已变化，请重新读取 assistant.get 后重新诊断，未修改任何参数".into()); }
    for value in remove {
        tx.execute("DELETE FROM assistant_model_config WHERE assistant_id=?1 AND name=?2", params![assistant, value.as_str().unwrap()]).map_err(|e| e.to_string())?;
    }
    for (name, value) in set {
        let text = value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string());
        tx.execute("DELETE FROM assistant_model_config WHERE assistant_id=?1 AND name=?2", params![assistant, name]).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO assistant_model_config(assistant_id,assistant_model_id,name,value,value_type) VALUES(?1,?2,?3,?4,?5)", params![assistant,model,name,text,value_type(name,value)?]).map_err(|e| e.to_string())?;
    }
    let after = snapshot(&tx, assistant)?;
    if !dry_run { tx.commit().map_err(|e| e.to_string())?; }
    Ok(json!({"assistant_id":assistant,"dry_run":dry_run,"before":before,"model_configs":after,
        "instruction":"配置在下一轮请求生效；这不是任务成功证明。核对原错误与工作归属后，使用原助手继续原工作；不更换模型或通道。"}))
}

struct Handler;
#[async_trait]
impl ActionHandler for Handler {
    async fn execute(&self, app: &AppHandle, args: Value, dry_run: bool) -> Result<Value,String> {
        let mut db = AssistantDatabase::new(app).map_err(|e| e.to_string())?;
        patch(&mut db.conn, &args, dry_run)
    }
    async fn snapshot_before(&self, _app: &AppHandle, args: &Value) -> Option<Value> {
        // A successful patch guarantees this is the exact pre-state under the write lock.
        // Reading again here would introduce a race between audit snapshot and execution.
        Some(json!({"configs":args.get("expected_configs")?.as_array()?}))
    }
    async fn undo(&self, app: &AppHandle, before: &Value, args: &Value) -> Result<Value,String> {
        let mut db = AssistantDatabase::new(app).map_err(|e| e.to_string())?;
        undo_patch(&mut db.conn, before, args)
    }
}

fn undo_patch(conn: &mut Connection, before: &Value, args: &Value) -> Result<Value,String> {
        let assistant = args["assistant_id"].as_i64().ok_or("缺少助手 ID")?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
        let current = snapshot(&tx,assistant)?;
        let old = before["configs"].as_array().ok_or("撤销快照缺少参数")?;
        // Only restore touched keys; refuse to overwrite later edits to those keys.
        let set = args["set"].as_object().ok_or("缺少 set")?;
        let remove = args["remove"].as_array().ok_or("缺少 remove")?;
        for name in set.keys().map(String::as_str).chain(remove.iter().filter_map(Value::as_str)) {
            let row = current.as_array().unwrap().iter().find(|r| r["name"] == name);
            let expected = set.get(name).map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()));
            if (expected.is_none() && row.is_some()) || row.and_then(|r| r["value"].as_str()) != expected.as_deref() || row.is_some_and(|r| r["assistant_model_id"] != args["assistant_model_id"] || set.get(name).is_some_and(|v| value_type(name,v).ok() != r["value_type"].as_str())) { return Err(format!("参数 {name} 已被修改，不能覆盖撤销")); }
            tx.execute("DELETE FROM assistant_model_config WHERE assistant_id=?1 AND name=?2",params![assistant,name]).map_err(|e| e.to_string())?;
            if let Some(old) = old.iter().find(|r| r["name"] == name) {
                tx.execute("INSERT INTO assistant_model_config(assistant_id,assistant_model_id,name,value,value_type) VALUES(?1,?2,?3,?4,?5)",params![assistant,old["assistant_model_id"].as_i64(),name,old["value"].as_str(),old["value_type"].as_str()]).map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(json!({"undone":true,"assistant_id":assistant}))
}

pub fn register(registry: &mut ActionRegistry) {
    registry.register(ActionMeta {
        action_id:"assistant.patch_model_config".into(), domain:"assistant".into(),
        summary:"修复助手模型请求参数".into(),
        description:"先 assistant.get 读取绑定及 model_configs，再按具体错误最小修改。参数为助手级共享配置；set 设置，remove 删除（不再发送），null 不等于删除。不会更换模型/provider/通道，也不自动重跑。".into(),
        risk_level:RiskLevel::MEDIUM, requires_approval:false, approval_policy:ApprovalPolicy::AllowInScope,
        allowed_scopes:vec![ActionScope::Assistant], tags:vec!["assistant".into(),"write".into(),"model".into(),"repair".into()],
        args_schema:json!({"type":"object","properties":{
            "assistant_id":{"type":"integer"},"assistant_model_id":{"type":"integer"},
            "expected_configs":{"type":"array","description":"assistant.get 返回的完整 model_configs，原样传入"},
            "set":{"type":"object","description":"temperature/top_p 数字、max_tokens 正整数、reasoning_effort 字符串"},
            "remove":{"type":"array","items":{"type":"string"}}
        },"required":["assistant_id","assistant_model_id","expected_configs","set","remove"]}),
        result_schema:json!({"type":"object"}), supports_dry_run:true,
        rollback_hint:Some("使用 superadmin_undo 撤销；被后续修改的参数不会覆盖".into()),
    },Box::new(Handler));
}
