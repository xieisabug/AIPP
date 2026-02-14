use std::path::PathBuf;

use assistant_db::AssistantDatabase;
use conversation_db::ConversationDatabase;
use llm_db::LLMDatabase;
use rusqlite::params;
use semver::Version;
use scheduled_task_db::ScheduledTaskDatabase;
use sub_task_db::SubTaskDatabase;
use system_db::SystemDatabase;
use tauri::Manager;
use tracing::{debug, error, info, instrument, warn};

pub mod assistant_db;
pub mod conversation_db;
pub mod llm_db;
pub mod mcp_db;
pub mod plugin_db;
pub mod scheduled_task_db;
pub mod skill_db;
pub mod sub_task_db;
pub mod system_db;

#[cfg(test)]
mod tests;

const CURRENT_VERSION: &str = "0.0.9";

pub(crate) fn get_db_path(app_handle: &tauri::AppHandle, db_name: &str) -> Result<PathBuf, String> {
    let app_dir = app_handle.path().app_data_dir().unwrap();
    let db_path = app_dir.join("db");
    std::fs::create_dir_all(&db_path).map_err(|e| e.to_string())?;
    Ok(db_path.join(db_name))
}

#[instrument(level = "info", skip(app_handle, system_db, llm_db, assistant_db, conversation_db))]
pub fn database_upgrade(
    app_handle: &tauri::AppHandle,
    system_db: SystemDatabase,
    llm_db: LLMDatabase,
    assistant_db: AssistantDatabase,
    conversation_db: ConversationDatabase,
) -> Result<(), String> {
    info!(target_version = CURRENT_VERSION, "Starting database upgrade check");
    let system_version = system_db.get_config("system_version");
    match system_version {
        Ok(version) => {
            if version.is_empty() {
                let _ = system_db.add_system_config("system_version", CURRENT_VERSION);
                info!("Initialized system_version to current version");
                if let Err(err) = system_db.init_feature_config() {
                    error!(error = ?err, "init_feature_config failed");
                } else {
                    info!("Feature configs initialized");
                }
            } else {
                // 临时逻辑
                let now_version;
                if version == "0.1" {
                    let _ = system_db.delete_system_config("system_version");
                    let _ = system_db.add_system_config("system_version", "0.0.1");
                    now_version = "0.0.1";
                } else {
                    now_version = version.as_str();
                }
                info!(current_version = now_version, "Detected existing system version");

                let current_version = Version::parse(now_version).unwrap();

                // 定义需要执行特殊逻辑的版本
                let special_versions: Vec<(
                    &str,
                    fn(
                        &SystemDatabase,
                        &LLMDatabase,
                        &AssistantDatabase,
                        &ConversationDatabase,
                        &tauri::AppHandle,
                    ) -> Result<(), String>,
                )> = vec![
                    ("0.0.2", special_logic_0_0_2),
                    ("0.0.3", special_logic_0_0_3),
                    ("0.0.4", special_logic_0_0_4),
                    ("0.0.5", special_logic_0_0_5),
                    ("0.0.6", special_logic_0_0_6),
                    ("0.0.7", special_logic_0_0_7),
                    ("0.0.8", special_logic_0_0_8),
                    ("0.0.9", special_logic_0_0_9),
                ];

                for (version_str, logic) in special_versions.iter() {
                    let version = Version::parse(version_str).unwrap();
                    if current_version < version {
                        info!(target = version_str, "Executing special logic for version");
                        let start = std::time::Instant::now();
                        match logic(
                            &system_db,
                            &llm_db,
                            &assistant_db,
                            &conversation_db,
                            app_handle,
                        ) {
                            Ok(_) => {
                                info!(
                                    target = version_str,
                                    elapsed_ms = start.elapsed().as_millis() as u64,
                                    "Special logic completed"
                                );
                            }
                            Err(err) => {
                                error!(target = version_str, error = ?err, "Special logic failed; exiting");
                                app_handle.exit(-1);
                            }
                        }
                    } else {
                        debug!(
                            target = version_str,
                            "Skipping special logic; already at or past version"
                        );
                    }
                }

                let _ = system_db.update_system_config("system_version", CURRENT_VERSION);
                info!("System version updated to current");
            }
        }
        Err(err) => {
            error!(error = ?err, "Failed to read system_version");
        }
    }

    Ok(())
}

fn special_logic_0_0_2(
    _system_db: &SystemDatabase,
    llm_db: &LLMDatabase,
    assistant_db: &AssistantDatabase,
    _conversation_db: &ConversationDatabase,
    _app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    info!("Running special_logic_0_0_2");
    // 开始事务
    assistant_db
        .conn
        .execute("BEGIN TRANSACTION;", [])
        .map_err(|e| format!("添加字段model_code失败: {}", e.to_string()))?;

    // 添加新字段
    assistant_db
        .conn
        .execute(
            "ALTER TABLE assistant_model ADD COLUMN provider_id INTEGER NOT NULL DEFAULT 0;",
            [],
        )
        .map_err(|e| format!("添加字段provider_id失败: {}", e.to_string()))?;
    assistant_db
        .conn
        .execute("ALTER TABLE assistant_model ADD COLUMN model_code TEXT NOT NULL DEFAULT '';", [])
        .map_err(|e| format!("添加字段model_code失败: {}", e.to_string()))?;
    assistant_db
        .conn
        .execute(
            "ALTER TABLE assistant_model_config ADD COLUMN value_type TEXT NOT NULL DEFAULT 'float';",
            [],
        )
        .map_err(|e| format!("添加字段value_type失败: {}", e.to_string()))?;

    assistant_db
        .conn
        .execute(
            "UPDATE assistant_model_config SET value_type = 'boolean' WHERE name = 'stream';",
            [],
        )
        .map_err(|e| format!("更新stream类型失败: {}", e.to_string()))?;
    assistant_db
        .conn
        .execute(
            "UPDATE assistant_model_config SET value_type = 'number' WHERE name = 'max_tokens';",
            [],
        )
        .map_err(|e| format!("更新max_tokens类型失败: {}", e.to_string()))?;

    // 查询所有 model_id
    let mut stmt = assistant_db
        .conn
        .prepare("SELECT model_id FROM assistant_model")
        .map_err(|e| format!("查询助手模型失败: {}", e.to_string()))?;
    let model_ids_iter = stmt
        .query_map([], |row| row.get::<_, i64>(0))
        .map_err(|e| format!("助手模型id转i64失败: {}", e.to_string()))?;

    for model_id_result in model_ids_iter {
        let model_id = model_id_result.map_err(|e| e.to_string())?;

        if let Ok(model) = llm_db.get_llm_model_detail_by_id(&model_id) {
            // 处理查询到的 model
            // 更新新字段
            assistant_db
                .conn
                .execute(
                    "UPDATE assistant_model SET provider_id = ?, model_code = ? WHERE model_id = ?;",
                    params![model.provider.id, model.model.code, model_id],
                )
                .map_err(|e| format!("更新助手模型失败: {}", e.to_string()))?;
        } else {
            // 查询不到结果，跳过这次循环
            continue;
        }
    }

    // 删除旧字段
    assistant_db
        .conn
        .execute("ALTER TABLE assistant_model DROP COLUMN model_id;", [])
        .map_err(|e| format!("删除model_id字段失败: {}", e.to_string()))?;

    // 提交事务
    assistant_db
        .conn
        .execute("COMMIT;", [])
        .map_err(|e| format!("事务提交失败: {}", e.to_string()))?;
    info!("special_logic_0_0_2 done");
    Ok(())
}

fn special_logic_0_0_4(
    _system_db: &SystemDatabase,
    _llm_db: &LLMDatabase,
    _assistant_db: &AssistantDatabase,
    conversation_db: &ConversationDatabase,
    _app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    info!("special_logic_0_0_4: 清理废弃的assistant消息类型");

    // 创建数据库连接
    let conn = conversation_db
        .get_connection()
        .map_err(|e| format!("打开数据库连接失败: {}", e.to_string()))?;

    // 开始事务
    conn.execute("BEGIN TRANSACTION;", [])
        .map_err(|e| format!("开始事务失败: {}", e.to_string()))?;

    // 查询所有废弃的assistant类型消息
    let mut stmt = conn
        .prepare("SELECT id, content FROM message WHERE message_type = 'assistant'")
        .map_err(|e| format!("查询废弃消息失败: {}", e.to_string()))?;

    let deprecated_messages = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,    // id
                row.get::<_, String>(1)?, // content
            ))
        })
        .map_err(|e| format!("查询废弃消息失败: {}", e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("收集废弃消息数据失败: {}", e.to_string()))?;

    info!(deprecated_count = deprecated_messages.len(), "发现废弃assistant消息");

    // 将废弃的assistant消息转换为response消息
    for (message_id, _content) in deprecated_messages {
        // 检查是否有对应的reasoning消息（通过时间相近来判断）
        let mut reasoning_stmt = conn
            .prepare("SELECT id FROM message WHERE message_type = 'reasoning' AND conversation_id = (SELECT conversation_id FROM message WHERE id = ?) AND ABS(julianday(created_time) - julianday((SELECT created_time FROM message WHERE id = ?))) < 0.001")
            .map_err(|e| format!("查询对应reasoning消息失败: {}", e.to_string()))?;

        let reasoning_exists =
            reasoning_stmt.query_row([message_id, message_id], |_| Ok(())).is_ok();

        if reasoning_exists {
            // 如果有reasoning消息，生成一个generation_group_id来关联它们
            let generation_group_id = uuid::Uuid::new_v4().to_string();

            // 更新reasoning消息的generation_group_id
            conn.execute(
                "UPDATE message SET generation_group_id = ? WHERE message_type = 'reasoning' AND conversation_id = (SELECT conversation_id FROM message WHERE id = ?) AND ABS(julianday(created_time) - julianday((SELECT created_time FROM message WHERE id = ?))) < 0.001",
                params![generation_group_id, message_id, message_id],
            )
            .map_err(|e| format!("更新reasoning消息generation_group_id失败: {}", e.to_string()))?;

            // 更新assistant消息为response类型并设置generation_group_id
            conn.execute(
                "UPDATE message SET message_type = 'response', generation_group_id = ? WHERE id = ?",
                params![generation_group_id, message_id],
            )
            .map_err(|e| format!("更新assistant消息失败: {}", e.to_string()))?;
        } else {
            // 如果没有reasoning消息，直接转换为response并生成新的generation_group_id
            let generation_group_id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "UPDATE message SET message_type = 'response', generation_group_id = ? WHERE id = ?",
                params![generation_group_id, message_id],
            )
            .map_err(|e| format!("更新单独assistant消息失败: {}", e.to_string()))?;
        }
    }

    // 验证清理结果
    let remaining_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM message WHERE message_type = 'assistant'", [], |row| {
            row.get(0)
        })
        .map_err(|e| format!("验证清理结果失败: {}", e.to_string()))?;

    if remaining_count > 0 {
        return Err(format!("清理未完成，仍有 {} 条assistant消息", remaining_count));
    }

    // 提交事务
    conn.execute("COMMIT;", []).map_err(|e| format!("事务提交失败: {}", e.to_string()))?;

    info!("special_logic_0_0_4 done: 废弃的assistant消息类型已清理完成");
    Ok(())
}

fn special_logic_0_0_3(
    _system_db: &SystemDatabase,
    _llm_db: &LLMDatabase,
    _assistant_db: &AssistantDatabase,
    conversation_db: &ConversationDatabase,
    _app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    info!("special_logic_0_0_3: 添加 generation_group_id 字段并更新现有数据");

    // 创建数据库连接
    let conn = conversation_db
        .get_connection()
        .map_err(|e| format!("打开数据库连接失败: {}", e.to_string()))?;

    // 开始事务
    conn.execute("BEGIN TRANSACTION;", [])
        .map_err(|e| format!("开始事务失败: {}", e.to_string()))?;

    // 添加 generation_group_id 字段
    conn.execute("ALTER TABLE message ADD COLUMN generation_group_id TEXT;", [])
        .map_err(|e| format!("添加 generation_group_id 字段失败: {}", e.to_string()))?;

    // 更新现有数据：为reasoning和response消息配对生成generation_group_id
    // 首先查询所有需要更新的消息，按对话分组
    let mut stmt = conn
        .prepare(
            "SELECT id, conversation_id, message_type, created_time 
             FROM message 
             WHERE message_type IN ('reasoning', 'response') 
             ORDER BY conversation_id, created_time",
        )
        .map_err(|e| format!("准备查询消息失败: {}", e.to_string()))?;

    let message_rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,    // id
                row.get::<_, i64>(1)?,    // conversation_id
                row.get::<_, String>(2)?, // message_type
                row.get::<_, String>(3)?, // created_time
            ))
        })
        .map_err(|e| format!("查询消息失败: {}", e.to_string()))?;

    let messages: Vec<(i64, i64, String, String)> = message_rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("收集消息数据失败: {}", e.to_string()))?;

    // 按对话ID分组处理消息
    let mut conversation_messages: std::collections::HashMap<i64, Vec<(i64, String, String)>> =
        std::collections::HashMap::new();
    for (id, conversation_id, message_type, created_time) in messages {
        conversation_messages.entry(conversation_id).or_insert_with(Vec::new).push((
            id,
            message_type,
            created_time,
        ));
    }

    // 为每个对话的reasoning和response消息配对
    for (_conversation_id, mut msgs) in conversation_messages {
        // 按创建时间排序
        msgs.sort_by(|a, b| a.2.cmp(&b.2));

        let mut i = 0;
        while i < msgs.len() {
            let current_msg = &msgs[i];

            // 如果当前消息是reasoning，查找后续的response
            if current_msg.1 == "reasoning" {
                let mut found_response = false;
                let mut j = i + 1;

                // 查找同一个generation的response消息
                while j < msgs.len() {
                    let next_msg = &msgs[j];
                    if next_msg.1 == "response" {
                        // 找到配对的response，为这两条消息生成相同的generation_group_id
                        let generation_group_id = format!("{}", uuid::Uuid::new_v4());

                        // 更新reasoning消息
                        conn.execute(
                            "UPDATE message SET generation_group_id = ? WHERE id = ?",
                            params![generation_group_id, current_msg.0],
                        )
                        .map_err(|e| {
                            format!("更新reasoning消息generation_group_id失败: {}", e.to_string())
                        })?;

                        // 更新response消息
                        conn.execute(
                            "UPDATE message SET generation_group_id = ? WHERE id = ?",
                            params![generation_group_id, next_msg.0],
                        )
                        .map_err(|e| {
                            format!("更新response消息generation_group_id失败: {}", e.to_string())
                        })?;

                        found_response = true;
                        i = j + 1; // 跳过已处理的response消息
                        break;
                    }
                    j += 1;
                }

                if !found_response {
                    // 没有找到配对的response，为单独的reasoning生成generation_group_id
                    let generation_group_id = format!("{}", uuid::Uuid::new_v4());
                    conn.execute(
                        "UPDATE message SET generation_group_id = ? WHERE id = ?",
                        params![generation_group_id, current_msg.0],
                    )
                    .map_err(|e| {
                        format!("更新单独reasoning消息generation_group_id失败: {}", e.to_string())
                    })?;
                    i += 1;
                }
            } else if current_msg.1 == "response" {
                // 如果是单独的response消息（没有前面的reasoning），也生成generation_group_id
                let generation_group_id = format!("{}", uuid::Uuid::new_v4());
                conn.execute(
                    "UPDATE message SET generation_group_id = ? WHERE id = ?",
                    params![generation_group_id, current_msg.0],
                )
                .map_err(|e| {
                    format!("更新单独response消息generation_group_id失败: {}", e.to_string())
                })?;
                i += 1;
            } else {
                i += 1;
            }
        }
    }

    // 提交事务
    conn.execute("COMMIT;", []).map_err(|e| format!("事务提交失败: {}", e.to_string()))?;

    info!("special_logic_0_0_3 done: generation_group_id 字段添加完成，现有数据已更新");
    Ok(())
}

fn special_logic_0_0_5(
    _system_db: &SystemDatabase,
    _llm_db: &LLMDatabase,
    _assistant_db: &AssistantDatabase,
    _conversation_db: &ConversationDatabase,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    info!("special_logic_0_0_5: 创建 sub task 相关表");

    // 创建 SubTaskDatabase 实例
    let sub_task_db = SubTaskDatabase::new(app_handle)
        .map_err(|e| format!("创建 SubTaskDatabase 失败: {}", e.to_string()))?;

    // 创建 sub task 相关表
    sub_task_db.create_tables().map_err(|e| format!("创建 sub task 表失败: {}", e.to_string()))?;

    info!("special_logic_0_0_5 done: sub task 表创建完成");
    Ok(())
}

fn special_logic_0_0_6(
    _system_db: &SystemDatabase,
    _llm_db: &LLMDatabase,
    _assistant_db: &AssistantDatabase,
    _conversation_db: &ConversationDatabase,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    info!("special_logic_0_0_6: 创建 scheduled_task 表");

    let scheduled_task_db = ScheduledTaskDatabase::new(app_handle)
        .map_err(|e| format!("创建 ScheduledTaskDatabase 失败: {}", e.to_string()))?;
    scheduled_task_db
        .create_tables()
        .map_err(|e| format!("创建 scheduled_task 表失败: {}", e.to_string()))?;

    info!("special_logic_0_0_6 done: scheduled_task 表创建完成");
    Ok(())
}

fn special_logic_0_0_7(
    _system_db: &SystemDatabase,
    _llm_db: &LLMDatabase,
    _assistant_db: &AssistantDatabase,
    _conversation_db: &ConversationDatabase,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    info!("special_logic_0_0_7: 创建 scheduled_task_log 表");

    let scheduled_task_db = ScheduledTaskDatabase::new(app_handle)
        .map_err(|e| format!("创建 ScheduledTaskDatabase 失败: {}", e.to_string()))?;
    scheduled_task_db
        .create_tables()
        .map_err(|e| format!("创建 scheduled_task_log 表失败: {}", e.to_string()))?;

    info!("special_logic_0_0_7 done: scheduled_task_log 表创建完成");
    Ok(())
}

fn special_logic_0_0_8(
    _system_db: &SystemDatabase,
    _llm_db: &LLMDatabase,
    _assistant_db: &AssistantDatabase,
    conversation_db: &ConversationDatabase,
    _app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    info!("special_logic_0_0_8: 创建 conversation_todo 表");

    // The create_tables method will create the conversation_todo table if it doesn't exist
    conversation_db
        .create_tables()
        .map_err(|e| format!("创建 conversation_todo 表失败: {}", e.to_string()))?;

    info!("special_logic_0_0_8 done: conversation_todo 表创建完成");
    Ok(())
}

fn special_logic_0_0_9(
    system_db: &SystemDatabase,
    _llm_db: &LLMDatabase,
    _assistant_db: &AssistantDatabase,
    _conversation_db: &ConversationDatabase,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    info!("special_logic_0_0_9: 开始检查技能迁移");

    // 检查迁移标记（避免重复迁移）
    match system_db.get_config("skills_migrated_to_agents") {
        Ok(val) if val == "1" => {
            debug!("special_logic_0_0_9: 技能迁移标记已存在，跳过迁移");
            return Ok(());
        }
        Ok(val) => {
            debug!("special_logic_0_0_9: 迁移标记值为 '{}'，将执行迁移", val);
        }
        Err(e) => {
            debug!("special_logic_0_0_9: 读取迁移标记失败: {}，将执行迁移", e);
        }
    }

    // 执行迁移
    debug!("special_logic_0_0_9: 开始执行技能迁移");
    match crate::api::skill_api::migrate_skills_to_agents_dir(app_handle) {
        Ok(_) => {
            info!("special_logic_0_0_9: 技能迁移执行成功");
        }
        Err(e) => {
            error!("special_logic_0_0_9: 技能迁移执行失败: {}", e);
            // 迁移失败不阻止程序启动，记录错误但继续
        }
    }

    // 设置迁移标记（即使迁移失败也设置标记，避免下次重复尝试）
    debug!("special_logic_0_0_9: 设置迁移标记");
    match system_db.add_system_config("skills_migrated_to_agents", "1") {
        Ok(_) => {
            debug!("special_logic_0_0_9: 迁移标记设置成功");
        }
        Err(e) => {
            error!("special_logic_0_0_9: 设置迁移标记失败: {}", e);
            // 标记设置失败也不阻止程序启动
        }
    }

    info!("special_logic_0_0_9 done: 技能迁移流程完成");
    Ok(())
}
