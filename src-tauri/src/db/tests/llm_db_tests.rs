//! LLM Provider 和 Model 数据库操作测试
//!
//! ## 测试范围
//! - LLM Provider CRUD 操作
//! - LLM Model 操作
//! - LLM Provider Config 配置操作
//! - Model Detail 查询
//!
//! ## 测试隔离
//! 所有测试使用 `Connection::open_in_memory()` 创建内存数据库

use crate::db::llm_db::*;
use rusqlite::Connection;

// ============================================================================
// 测试辅助函数
// ============================================================================

/// 创建测试用内存数据库并初始化 LLM 相关表结构
///
/// **安全性**: 使用内存数据库，不会影响真实数据
fn create_llm_test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();

    // 创建 llm_provider 表
    conn.execute(
        "CREATE TABLE llm_provider (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            api_type TEXT NOT NULL,
            description TEXT,
            is_official BOOLEAN NOT NULL DEFAULT 0,
            is_enabled BOOLEAN NOT NULL DEFAULT 0,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )
    .unwrap();

    // 创建 llm_model 表
    conn.execute(
        "CREATE TABLE llm_model (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            llm_provider_id INTEGER NOT NULL,
            code TEXT NOT NULL,
            description TEXT,
            vision_support BOOLEAN NOT NULL DEFAULT 0,
            audio_support BOOLEAN NOT NULL DEFAULT 0,
            video_support BOOLEAN NOT NULL DEFAULT 0,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (llm_provider_id) REFERENCES llm_provider(id)
        )",
        [],
    )
    .unwrap();

    // 创建 llm_provider_config 表
    conn.execute(
        "CREATE TABLE llm_provider_config (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            llm_provider_id INTEGER NOT NULL,
            value TEXT,
            append_location TEXT DEFAULT 'header',
            is_addition BOOLEAN NOT NULL DEFAULT 0,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )
    .unwrap();

    conn
}

/// 创建 LLMDatabase 实例用于测试
fn create_llm_db() -> LLMDatabase {
    let conn = create_llm_test_db();
    LLMDatabase { conn }
}

// ============================================================================
// 正常情况测试
// ============================================================================

/// 测试 LLM Provider 的完整 CRUD 生命周期
///
/// 验证内容：
/// - Create: 创建 Provider 成功
/// - Read: 能够读取 Provider 信息
/// - Update: 修改 Provider 配置后持久化成功
/// - Delete: 删除 Provider 及其关联数据
#[test]
fn test_llm_provider_crud() {
    let db = create_llm_db();

    // Create
    db.add_llm_provider("OpenAI", "openai_api", "OpenAI API", true, true).unwrap();

    // Read list
    let providers = db.get_llm_providers().unwrap();
    assert_eq!(providers.len(), 1);
    let (id, name, api_type, desc, is_official, is_enabled) = &providers[0];
    assert_eq!(name, "OpenAI");
    assert_eq!(api_type, "openai_api");
    assert_eq!(desc, "OpenAI API");
    assert!(is_official);
    assert!(is_enabled);

    // Read single
    let provider = db.get_llm_provider(*id).unwrap();
    assert_eq!(provider.name, "OpenAI");

    // Update
    db.update_llm_provider(*id, "OpenAI Updated", "openai_api_v2", "Updated desc", false).unwrap();
    let updated = db.get_llm_provider(*id).unwrap();
    assert_eq!(updated.name, "OpenAI Updated");
    assert_eq!(updated.api_type, "openai_api_v2");
    assert!(!updated.is_enabled);

    // Delete
    db.delete_llm_provider(*id).unwrap();
    let providers_after = db.get_llm_providers().unwrap();
    assert!(providers_after.is_empty());
}

/// 测试 LLM Model 操作
///
/// 验证内容：
/// - 为 Provider 添加 Model
/// - 获取所有 Model
/// - 获取指定 Provider 的 Model
/// - 删除 Model
#[test]
fn test_llm_model_operations() {
    let db = create_llm_db();

    // 先创建 Provider
    db.add_llm_provider("OpenAI", "openai_api", "OpenAI API", true, true).unwrap();
    let providers = db.get_llm_providers().unwrap();
    let provider_id = providers[0].0;

    // 添加 Model
    db.add_llm_model("GPT-4", provider_id, "gpt-4", "GPT-4 Model", true, false, false).unwrap();
    db.add_llm_model(
        "GPT-4 Vision",
        provider_id,
        "gpt-4-vision",
        "GPT-4 with Vision",
        true,
        false,
        false,
    )
    .unwrap();

    // 获取所有 Model
    let all_models = db.get_all_llm_models().unwrap();
    assert_eq!(all_models.len(), 2);

    // 获取指定 Provider 的 Model
    let provider_models = db.get_llm_models(provider_id.to_string()).unwrap();
    assert_eq!(provider_models.len(), 2);

    // 验证 Model 属性
    let (model_id, name, llm_provider_id, code, _desc, vision, audio, video) = &all_models[0];
    assert!(model_id > &0);
    assert_eq!(name, "GPT-4");
    assert_eq!(llm_provider_id, &provider_id);
    assert_eq!(code, "gpt-4");
    assert!(vision);
    assert!(!audio);
    assert!(!video);

    // 删除 Model
    db.delete_llm_model(provider_id, "gpt-4".to_string()).unwrap();
    let models_after = db.get_all_llm_models().unwrap();
    assert_eq!(models_after.len(), 1);
}

/// 测试 LLM Provider Config 操作
///
/// 验证内容：
/// - 为 Provider 添加配置项
/// - 获取 Provider 的配置列表
/// - 更新配置值
#[test]
fn test_llm_provider_config_operations() {
    let db = create_llm_db();

    // 创建 Provider
    db.add_llm_provider("OpenAI", "openai_api", "OpenAI API", true, true).unwrap();
    let providers = db.get_llm_providers().unwrap();
    let provider_id = providers[0].0;

    // 添加配置
    db.add_llm_provider_config(provider_id, "api_key", "sk-xxx", "header", false).unwrap();
    db.add_llm_provider_config(provider_id, "base_url", "https://api.openai.com", "header", false)
        .unwrap();

    // 获取配置
    let configs = db.get_llm_provider_config(provider_id).unwrap();
    assert_eq!(configs.len(), 2);

    // 验证配置
    let api_key_config = configs.iter().find(|c| c.name == "api_key").unwrap();
    assert_eq!(api_key_config.value, "sk-xxx");
    assert_eq!(api_key_config.append_location, "header");

    // 更新配置
    db.update_llm_provider_config(provider_id, "api_key", "sk-new-key").unwrap();
    let updated_configs = db.get_llm_provider_config(provider_id).unwrap();
    let updated_key = updated_configs.iter().find(|c| c.name == "api_key").unwrap();
    assert_eq!(updated_key.value, "sk-new-key");
}

/// 测试 ACP provider 即使没有 llm_model 记录也能出现在助手模型选择列表
///
/// 验证内容：
/// - ACP 助手需要 provider 级别配置，不应因为缺少模型行而无法选择 provider
/// - 生成的占位 code 不影响 ACP 运行时，运行时只使用 provider_id 读取 ACP 配置
#[test]
fn test_filtered_models_for_acp_includes_provider_without_model() {
    let db = create_llm_db();

    db.add_llm_provider("Codex", "acp", "ACP", false, true).unwrap();
    let providers = db.get_llm_providers().unwrap();
    let provider_id = providers[0].0;

    let models = db.get_filtered_models_for_select(4).unwrap();

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].0, "Codex");
    assert_eq!(models[0].1, "acp");
    assert_eq!(models[0].2, 0);
    assert_eq!(models[0].3, provider_id);
}

/// 测试 Model Detail 查询
///
/// 验证内容：
/// - 通过 provider_id 和 model_code 获取完整 ModelDetail
/// - 通过 model_id 获取 ModelDetail
/// - ModelDetail 包含 Model、Provider、Configs
#[test]
fn test_llm_model_detail() {
    let db = create_llm_db();

    // 创建 Provider 和 Model
    db.add_llm_provider("OpenAI", "openai_api", "OpenAI API", true, true).unwrap();
    let providers = db.get_llm_providers().unwrap();
    let provider_id = providers[0].0;

    db.add_llm_model("GPT-4", provider_id, "gpt-4", "GPT-4 Model", true, false, false).unwrap();
    db.add_llm_provider_config(provider_id, "api_key", "sk-xxx", "header", false).unwrap();

    // 通过 provider_id 和 code 获取
    let detail = db.get_llm_model_detail(&provider_id, &"gpt-4".to_string()).unwrap();
    assert_eq!(detail.model.name, "GPT-4");
    assert_eq!(detail.model.code, "gpt-4");
    assert_eq!(detail.provider.name, "OpenAI");
    assert_eq!(detail.configs.len(), 1);

    // 通过 model_id 获取
    let models = db.get_all_llm_models().unwrap();
    let model_id = models[0].0;
    let detail_by_id = db.get_llm_model_detail_by_id(&model_id).unwrap();
    assert_eq!(detail_by_id.model.name, "GPT-4");
}

/// 测试删除 Provider 时级联删除 Model
///
/// 验证内容：
/// - 删除 Provider 会同时删除其关联的 Model 和 Config
#[test]
fn test_llm_provider_cascade_delete() {
    let db = create_llm_db();

    // 创建 Provider、Model 和 Config
    db.add_llm_provider("OpenAI", "openai_api", "OpenAI API", true, true).unwrap();
    let providers = db.get_llm_providers().unwrap();
    let provider_id = providers[0].0;

    db.add_llm_model("GPT-4", provider_id, "gpt-4", "GPT-4", false, false, false).unwrap();
    db.add_llm_provider_config(provider_id, "api_key", "sk-xxx", "header", false).unwrap();

    // 删除 Provider
    db.delete_llm_provider(provider_id).unwrap();

    // 验证 Model 和 Config 也被删除
    let models = db.get_all_llm_models().unwrap();
    assert!(models.is_empty());

    let configs = db.get_llm_provider_config(provider_id).unwrap();
    assert!(configs.is_empty());
}

/// 测试多 Provider 场景
///
/// 验证内容：
/// - 可以添加多个 Provider
/// - 各 Provider 的 Model 相互独立
#[test]
fn test_multiple_providers() {
    let db = create_llm_db();

    // 创建多个 Provider
    db.add_llm_provider("OpenAI", "openai_api", "OpenAI", true, true).unwrap();
    db.add_llm_provider("Anthropic", "anthropic", "Anthropic", true, true).unwrap();

    let providers = db.get_llm_providers().unwrap();
    assert_eq!(providers.len(), 2);

    // 为各 Provider 添加 Model
    let openai_id = providers.iter().find(|p| p.1 == "OpenAI").unwrap().0;
    let anthropic_id = providers.iter().find(|p| p.1 == "Anthropic").unwrap().0;

    db.add_llm_model("GPT-4", openai_id, "gpt-4", "GPT-4", false, false, false).unwrap();
    db.add_llm_model("Claude", anthropic_id, "claude-3", "Claude 3", false, false, false).unwrap();

    // 验证 Model 属于正确的 Provider
    let openai_models = db.get_llm_models(openai_id.to_string()).unwrap();
    assert_eq!(openai_models.len(), 1);
    assert_eq!(openai_models[0].1, "GPT-4");

    let anthropic_models = db.get_llm_models(anthropic_id.to_string()).unwrap();
    assert_eq!(anthropic_models.len(), 1);
    assert_eq!(anthropic_models[0].1, "Claude");
}

// ============================================================================
// 异常和边界情况测试
// ============================================================================

/// 测试读取不存在的 Provider
///
/// 验证内容：
/// - 读取不存在的 ID 返回 QueryReturnedNoRows 错误
#[test]
fn test_llm_provider_read_nonexistent() {
    let db = create_llm_db();

    let result = db.get_llm_provider(999);
    assert!(result.is_err());
    match result {
        Err(rusqlite::Error::QueryReturnedNoRows) => {}
        _ => panic!("Expected QueryReturnedNoRows error"),
    }
}

/// 测试删除不存在的 Provider
///
/// 验证内容：
/// - 删除不存在的 ID 不会产生错误
#[test]
fn test_llm_provider_delete_nonexistent() {
    let db = create_llm_db();

    let result = db.delete_llm_provider(999);
    assert!(result.is_ok());
}

/// 测试查询不存在 Provider 的 Model
///
/// 验证内容：
/// - 查询不存在 provider_id 的 Model 返回空列表
#[test]
fn test_llm_model_nonexistent_provider() {
    let db = create_llm_db();

    let models = db.get_llm_models("999".to_string()).unwrap();
    assert!(models.is_empty());
}

/// 测试查询不存在 Provider 的 Config
///
/// 验证内容：
/// - 查询不存在 provider_id 的 Config 返回空列表
#[test]
fn test_llm_config_nonexistent_provider() {
    let db = create_llm_db();

    let configs = db.get_llm_provider_config(999).unwrap();
    assert!(configs.is_empty());
}

/// 测试查询不存在的 Model Detail
///
/// 验证内容：
/// - 通过不存在的 provider_id 和 code 查询返回错误
/// - 通过不存在的 model_id 查询返回错误
#[test]
fn test_llm_model_detail_nonexistent() {
    let db = create_llm_db();

    // 不存在的 provider + code
    let result1 = db.get_llm_model_detail(&999, &"nonexistent".to_string());
    assert!(result1.is_err());

    // 不存在的 model_id
    let result2 = db.get_llm_model_detail_by_id(&999);
    assert!(result2.is_err());
}

/// 测试空名称的 Provider
///
/// 验证内容：
/// - 空名称仍可以成功创建（数据库层不做业务验证）
#[test]
fn test_llm_provider_empty_name() {
    let db = create_llm_db();

    db.add_llm_provider("", "", "", false, false).unwrap();
    let providers = db.get_llm_providers().unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].1, "");
}

/// 测试超长名称的 Provider 和 Model
///
/// 验证内容：
/// - 超长名称可以正确存储和读取
#[test]
fn test_llm_very_long_names() {
    let db = create_llm_db();

    let long_name = "P".repeat(10000);
    let long_desc = "D".repeat(10000);

    db.add_llm_provider(&long_name, "api", &long_desc, false, true).unwrap();
    let providers = db.get_llm_providers().unwrap();
    assert_eq!(providers[0].1.len(), 10000);
    assert_eq!(providers[0].3.len(), 10000);

    // 长 Model 名称
    let provider_id = providers[0].0;
    let long_model_name = "M".repeat(10000);
    db.add_llm_model(&long_model_name, provider_id, "code", "desc", false, false, false).unwrap();

    let models = db.get_all_llm_models().unwrap();
    assert_eq!(models[0].1.len(), 10000);
}

/// 测试特殊字符在 Provider 和 Model 中的处理
///
/// 验证内容：
/// - 中文、Emoji 能正确存储
/// - SQL 注入字符被正确转义
#[test]
fn test_llm_special_characters() {
    let db = create_llm_db();

    // 中文和 Emoji
    db.add_llm_provider("智谱 🤖", "zhipu", "智谱清言 ✨", true, true).unwrap();
    let providers = db.get_llm_providers().unwrap();
    assert_eq!(providers[0].1, "智谱 🤖");
    assert_eq!(providers[0].3, "智谱清言 ✨");

    // SQL 注入尝试
    db.add_llm_provider("'; DROP TABLE llm_provider; --", "sql", "Injection", false, false)
        .unwrap();
    let providers_after = db.get_llm_providers().unwrap();
    assert_eq!(providers_after.len(), 2); // 表还存在
}

/// 测试删除不存在的 Model
///
/// 验证内容：
/// - 删除不存在的 Model 不会产生错误
#[test]
fn test_llm_model_delete_nonexistent() {
    let db = create_llm_db();

    let result = db.delete_llm_model(999, "nonexistent".to_string());
    assert!(result.is_ok());
}

/// 测试按 Provider 删除 Model
///
/// 验证内容：
/// - 删除指定 Provider 的所有 Model
/// - 其他 Provider 的 Model 不受影响
#[test]
fn test_llm_model_delete_by_provider() {
    let db = create_llm_db();

    // 创建两个 Provider
    db.add_llm_provider("Provider1", "api1", "Desc1", false, true).unwrap();
    db.add_llm_provider("Provider2", "api2", "Desc2", false, true).unwrap();
    let providers = db.get_llm_providers().unwrap();
    let p1_id = providers[0].0;
    let p2_id = providers[1].0;

    // 分别添加 Model
    db.add_llm_model("Model1", p1_id, "m1", "Desc", false, false, false).unwrap();
    db.add_llm_model("Model2", p2_id, "m2", "Desc", false, false, false).unwrap();

    // 删除 Provider1 的所有 Model
    db.delete_llm_model_by_provider(p1_id).unwrap();

    // 验证
    let all_models = db.get_all_llm_models().unwrap();
    assert_eq!(all_models.len(), 1);
    assert_eq!(all_models[0].1, "Model2");
}

/// 测试 Model 的多媒体支持标志
///
/// 验证内容：
/// - vision_support, audio_support, video_support 正确存储
#[test]
fn test_llm_model_media_support_flags() {
    let db = create_llm_db();

    db.add_llm_provider("Provider", "api", "Desc", false, true).unwrap();
    let providers = db.get_llm_providers().unwrap();
    let provider_id = providers[0].0;

    // 不同的多媒体支持组合
    db.add_llm_model("Text Only", provider_id, "text", "Text", false, false, false).unwrap();
    db.add_llm_model("Vision", provider_id, "vision", "Vision", true, false, false).unwrap();
    db.add_llm_model("Audio", provider_id, "audio", "Audio", false, true, false).unwrap();
    db.add_llm_model("All", provider_id, "all", "All", true, true, true).unwrap();

    let models = db.get_all_llm_models().unwrap();
    assert_eq!(models.len(), 4);

    // 验证各 Model 的多媒体支持
    let text_model = models.iter().find(|m| m.3 == "text").unwrap();
    assert!(!text_model.5 && !text_model.6 && !text_model.7);

    let vision_model = models.iter().find(|m| m.3 == "vision").unwrap();
    assert!(vision_model.5 && !vision_model.6 && !vision_model.7);

    let audio_model = models.iter().find(|m| m.3 == "audio").unwrap();
    assert!(!audio_model.5 && audio_model.6 && !audio_model.7);

    let all_model = models.iter().find(|m| m.3 == "all").unwrap();
    assert!(all_model.5 && all_model.6 && all_model.7);
}
