use super::*;

pub(super) fn databases() -> (SystemDatabase, LLMDatabase) {
    let sys = SystemDatabase { conn: Connection::open_in_memory().unwrap() };
    sys.create_tables().unwrap();
    let llm = LLMDatabase { conn: Connection::open_in_memory().unwrap() };
    llm.create_tables().unwrap();
    // Legacy installations may still contain these disabled providers.
    llm.conn.execute_batch("INSERT INTO llm_provider(id,name,api_type,description,is_official) VALUES
        (1,'OpenAI','openai_api','',1),(10,'Ollama','ollama','',1),
        (20,'Anthropic','anthropic','',1),(30,'DeepSeek','deepseek','',1)").unwrap();
    (sys, llm)
}

#[test]
fn test_manual_entry_requires_both_provider_and_assistant_to_be_unconfigured() {
    let (_, llm) = databases();
    let assistants = AssistantDatabase {
        conn: Connection::open_in_memory().unwrap(),
        mcp_conn: Connection::open_in_memory().unwrap(),
    };
    assistants.create_tables().unwrap();
    let transient = llm.add_llm_provider("草稿", "deepseek", "", false, false).unwrap();
    llm.mark_onboarding_draft(transient).unwrap();
    assert!(!has_manual_configuration(&llm, &assistants).unwrap());

    llm.release_onboarding_draft(transient).unwrap();
    assert!(has_manual_configuration(&llm, &assistants).unwrap());
    llm.conn.execute("DELETE FROM llm_provider WHERE id=?", [transient]).unwrap();
    llm.update_llm_provider_config(30, "api_key", "sample").unwrap();
    assert!(has_manual_configuration(&llm, &assistants).unwrap());
    llm.conn.execute("DELETE FROM llm_provider_config", []).unwrap();
    assistants.add_assistant_model(1, 30, "chat", "").unwrap();
    assert!(has_manual_configuration(&llm, &assistants).unwrap());
    assistants.conn.execute("DELETE FROM assistant_model WHERE provider_id=30", []).unwrap();
    assert!(!has_manual_configuration(&llm, &assistants).unwrap());
    assistants.add_assistant("我的助手", "", Some(0), false).unwrap();
    assert!(has_manual_configuration(&llm, &assistants).unwrap());
}

#[test]
fn test_startup_requires_enabled_chat_model_and_credentials() {
    let (sys, llm) = databases();
    assert!(should_show(&sys, &llm).unwrap());
    llm.add_llm_model("chat", 30, "chat", "", false, false, false).unwrap();
    llm.conn.execute("UPDATE llm_provider SET is_enabled=1 WHERE id=30", []).unwrap();
    assert!(should_show(&sys, &llm).unwrap());
    llm.update_llm_provider_config(30, "api_key", "sample").unwrap();
    assert!(!should_show(&sys, &llm).unwrap());
    assert_eq!(local(&sys, "onboarding_state").unwrap().as_deref(), Some("completed"));
}

#[test]
fn test_sync_checks_installed_agent_and_ollama_needs_no_key() {
    let (sys, llm) = databases();
    let id = llm.add_llm_provider("Codex", "codex_app_server", "", false, true).unwrap();
    assert!(matches!(sources(&llm, Some(&[])).unwrap(), Source::MissingAgent));
    assert!(matches!(sources(&llm, Some(&[id])).unwrap(), Source::Usable));
    assert!(!should_show(&sys, &llm).unwrap());
    llm.add_llm_model("small", 10, "small", "", false, false, false).unwrap();
    llm.conn.execute("UPDATE llm_provider SET is_enabled=1 WHERE id=10", []).unwrap();
    assert!(matches!(sources(&llm, Some(&[])).unwrap(), Source::Usable));
}

#[test]
fn test_startup_prioritizes_apply_recovery_over_complete_butler() {
    let (sys, llm) = databases();
    put(&sys.conn, "onboarding_state", "in_progress").unwrap();
    llm.add_llm_provider("Codex", "codex_app_server", "", false, true).unwrap();
    for step in ["path_chosen", "awaiting_recharge", "source_ready"] {
        put(&sys.conn, "onboarding_step", step).unwrap();
        assert!(should_show(&sys, &llm).unwrap());
    }
    sys.conn.execute_batch("INSERT INTO feature_config(feature_code,key,value,data_type) VALUES
        ('experimental','butler_model_id','acp%%32','string'),('experimental','butler_main_workspace_path','unavailable-now','string'),
        ('experimental','butler_experiment_enabled','false','string')").unwrap();
    assert!(should_show(&sys, &llm).unwrap());
    put(&sys.conn, "onboarding_step", "path_chosen").unwrap();
    assert!(!should_show(&sys, &llm).unwrap());
    assert_eq!(local(&sys, "onboarding_state").unwrap().as_deref(), Some("completed"));
    put(&sys.conn, "onboarding_step", "applied").unwrap();
    assert!(!should_show(&sys, &llm).unwrap());
}

#[test]
fn test_startup_skipped_and_completed_do_not_reopen() {
    let (sys, llm) = databases();
    for state in ["skipped", "completed"] {
        put(&sys.conn, "onboarding_state", state).unwrap();
        assert!(!should_show(&sys, &llm).unwrap());
    }
}

#[test]
fn test_cleanup_protects_reused_and_recharge_sources() {
    let (sys, llm) = databases();
    let transient = llm.add_llm_provider("草稿", "openai_api", "", false, false).unwrap();
    let recharge = llm.add_llm_provider("待充值", "deepseek", "", false, false).unwrap();
    llm.mark_onboarding_draft(transient).unwrap();
    llm.mark_onboarding_draft(recharge).unwrap();
    let orphan = llm.add_llm_provider("未登记的草稿", "deepseek", "", false, false).unwrap();
    llm.mark_onboarding_draft(orphan).unwrap();
    let adopted = llm.add_llm_provider("用户启用", "deepseek", "", false, false).unwrap();
    llm.mark_onboarding_draft(adopted).unwrap();
    llm.conn.execute("UPDATE llm_provider SET is_enabled=1 WHERE id=?", [adopted]).unwrap();
    llm.conn.execute("UPDATE llm_provider SET is_enabled=0 WHERE id=?", [adopted]).unwrap();
    let preexisting = llm.add_llm_provider("原先的设置", "deepseek", "", false, false).unwrap();
    let mut d = Draft::default();
    for (id, owned) in [(30, false), (transient, true), (recharge, true), (preexisting, true)] {
        d.providers.push(DraftProvider { provider_id: id, owned });
    }
    clear_unverified(&sys, &llm, &mut d, Some(recharge)).unwrap();
    assert!(llm.get_llm_provider(transient).is_err());
    assert!(llm.get_llm_provider(orphan).is_err());
    assert!(llm.get_llm_provider(adopted).is_ok());
    assert!(llm.get_llm_provider(preexisting).is_ok());
    assert!(llm.get_llm_provider(30).is_ok());
    assert!(llm.get_llm_provider(recharge).is_ok());
    assert_eq!(d.providers.len(), 1);
}

#[test]
fn test_source_uses_copilot_oauth_credential_and_magpie_default() {
    let (_, llm) = databases();
    let copilot = llm.add_llm_provider("Copilot", "github_copilot", "", false, true).unwrap();
    llm.add_llm_model("chat", copilot, "chat", "", false, false, false).unwrap();
    assert!(matches!(sources(&llm, Some(&[])).unwrap(), Source::MissingKey));
    llm.update_llm_provider_config(copilot, "api_key", "test-oauth-token").unwrap();
    assert!(matches!(sources(&llm, Some(&[])).unwrap(), Source::Usable));
    llm.conn.execute("UPDATE llm_provider SET is_enabled=0 WHERE id=?", [copilot]).unwrap();
    let magpie = llm.add_llm_provider("本机网关", "magpie", "", false, true).unwrap();
    llm.add_llm_model("chat", magpie, "chat", "", false, false, false).unwrap();
    assert!(matches!(sources(&llm, Some(&[])).unwrap(), Source::Usable));
}
