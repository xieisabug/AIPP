use super::*;

fn config(id: i64) -> Config {
    serde_json::from_value(
        serde_json::json!({"chat":{"providerId":id,"code":"chat","verified":true},
       "light":{"providerId":id,"code":"small","verified":true},"home":"butler_experiment",
       "models":[{"providerId":id,"codes":["chat","small"]}]}),
    )
    .unwrap()
}

#[test]
fn test_apply_seed_reuse_replays_without_duplicates() {
    let (sys, llm) = super::super::state_tests::databases();
    let id = llm.add_llm_provider("DeepSeek", "deepseek", "", false, false).unwrap();
    llm.mark_onboarding_draft(id).unwrap();
    llm.update_llm_provider_config(id, "api_key", "test").unwrap();
    llm.update_llm_provider_config(id, "endpoint", "https://example.invalid").unwrap();
    let mut c = config(id);
    let mut d = Draft {
        providers: vec![DraftProvider { provider_id: id, owned: true }],
        config: Some(c.clone()),
        ..Default::default()
    };
    prepare_providers(&sys, &llm, &mut d, &mut c, None).unwrap();
    assert_eq!(c.chat.provider_id, 30);
    let mut saved = draft(&sys).unwrap();
    let mut replay = saved.config.clone().unwrap();
    prepare_providers(&sys, &llm, &mut saved, &mut replay, None).unwrap();
    assert_eq!(replay, c);
    assert_eq!(llm.get_llm_models("30".into()).unwrap().len(), 2);
    assert!(llm.get_llm_provider(id).is_err());
    assert!(llm.get_llm_provider(30).unwrap().is_enabled);
    assert_eq!(llm.get_llm_providers().unwrap().iter().filter(|p| p.2 == "deepseek").count(), 1);
    assert_eq!(
        llm.get_llm_provider_config(30)
            .unwrap()
            .iter()
            .find(|v| v.name == "endpoint")
            .unwrap()
            .value,
        "https://example.invalid"
    );
}

#[test]
fn test_apply_workspace_and_enable_are_one_partial_update() {
    let (mut sys, _) = super::super::state_tests::databases();
    let c = config(30);
    let path = if cfg!(windows) { r"C:\Work" } else { "/work" };
    let home = home_settings(&sys, &c, || Ok(path.into())).unwrap();
    assert_eq!(home["butler_experiment_enabled"], "true");
    assert_eq!(home["butler_main_workspace_path"], path);
    sys.conn.execute("INSERT INTO feature_config(feature_code,key,value,data_type) VALUES('experimental','butler_display_name','existing','string')",[]).unwrap();
    crate::api::system_api::persist_feature_settings(&mut sys, "experimental", home, true).unwrap();
    assert_eq!(experiment(&sys, "butler_display_name").unwrap(), "existing");
    let replay = home_settings(&sys, &c, || panic!("must preserve configured workspace")).unwrap();
    assert_eq!(replay["butler_main_workspace_path"], path);
    put(&sys.conn, "onboarding_state", "completed").unwrap();
    let different = if cfg!(windows) { r"D:\Picked" } else { "/picked" };
    let rerun = Config { workspace_path: Some(different.into()), ..c.clone() };
    let replay = home_settings(&sys, &rerun, || panic!("rerun already has a workspace")).unwrap();
    assert_eq!(replay["butler_main_workspace_path"], path);
    let chat = Config { home: "chat_ui".into(), ..c };
    assert_eq!(home_settings(&sys, &chat, || panic!("chat has no workspace")).unwrap().len(), 1);
}

#[test]
fn test_apply_unverified_auxiliary_roles_preserve_existing_values() {
    let (mut sys, _) = super::super::state_tests::databases();
    let ok = Role { provider_id: 30, code: "new".into(), verified: true };
    let failed = Role { verified: false, ..ok.clone() };
    let initial = HashMap::from([
        ("auto_review_model".into(), "existing".into()),
        ("auto_review_provider_id".into(), "20".into()),
        ("title_prompt".into(), "custom".into()),
    ]);
    crate::api::system_api::persist_feature_settings(
        &mut sys,
        "conversation_summary",
        initial,
        true,
    )
    .unwrap();
    let changes = role_settings(Some(&ok), Some(&failed), false, false, false);
    crate::api::system_api::persist_feature_settings(
        &mut sys,
        "conversation_summary",
        changes,
        true,
    )
    .unwrap();
    let value = |k| sys.get_feature_config("conversation_summary", k).unwrap().unwrap().value;
    assert_eq!(value("default_light_model"), "new%%30");
    assert_eq!(value("auto_review_model"), "existing");
    assert_eq!(value("auto_review_provider_id"), "20");
    assert_eq!(value("title_prompt"), "custom");
}

#[test]
fn test_apply_recovers_after_provider_copy_before_draft_checkpoint() {
    let (sys, llm) = super::super::state_tests::databases();
    let id = llm.add_llm_provider("暂存", "deepseek", "", false, false).unwrap();
    llm.mark_onboarding_draft(id).unwrap();
    llm.update_llm_provider_config(id, "api_key", "test").unwrap();
    let c = config(id);
    let d = Draft {
        providers: vec![DraftProvider { provider_id: id, owned: true }],
        seed_targets: BTreeMap::from([(id, 30)]),
        config: Some(c),
        ..Default::default()
    };
    save_draft(&sys.conn, &d).unwrap();
    put(&sys.conn, "onboarding_step", "source_ready").unwrap();
    let tx = llm.conn.unchecked_transaction().unwrap();
    merge_seed(&llm, id, 30).unwrap();
    llm.conn.execute("UPDATE llm_provider SET is_enabled=1 WHERE id=30", []).unwrap();
    tx.commit().unwrap();

    let mut persisted = draft(&sys).unwrap();
    let mut replay = persisted.config.clone().unwrap();
    prepare_providers(&sys, &llm, &mut persisted, &mut replay, None).unwrap();
    assert_eq!(replay.chat.provider_id, 30);
    assert_eq!(draft(&sys).unwrap().config.unwrap(), replay);
    assert!(llm.get_llm_provider(id).is_err());
    assert_eq!(llm.get_llm_models("30".into()).unwrap().len(), 2);
}

#[test]
fn test_apply_auxiliary_retry_leaves_unrelated_disabled_provider_and_existing_endpoint() {
    let (sys, llm) = super::super::state_tests::databases();
    llm.update_llm_provider_config(30, "endpoint", "https://preserve.invalid").unwrap();
    let id = llm.add_llm_provider("辅助来源", "deepseek", "", false, false).unwrap();
    llm.mark_onboarding_draft(id).unwrap();
    llm.update_llm_provider_config(id, "api_key", "test").unwrap();
    let mut c = config(1);
    let retry = Role { provider_id: id, code: "small".into(), verified: true };
    c.light = Some(retry.clone());
    let mut d = Draft {
        providers: vec![DraftProvider { provider_id: id, owned: true }],
        config: Some(c.clone()),
        ..Default::default()
    };
    prepare_providers(&sys, &llm, &mut d, &mut c, Some(&[retry])).unwrap();
    assert!(llm.get_llm_provider(id).unwrap().is_enabled);
    assert!(!llm.get_llm_provider(1).unwrap().is_enabled);
    assert!(!llm.get_llm_provider(30).unwrap().is_enabled);
    assert_eq!(llm.get_llm_provider_config(30).unwrap()[0].value, "https://preserve.invalid");
}

#[test]
fn test_draft_notes_preserve_classification_without_raw_provider_diagnostic() {
    let mut note = RoleNote {
        choose_protocol: false,
        reason: Some("推荐表第 2 名".into()),
        error: Some("upstream echoed the request credential".into()),
    };
    note.sanitize();
    assert_eq!(note.reason.as_deref(), Some("推荐表第 2 名"));
    assert_eq!(note.error.as_deref(), Some("模型检查未通过，可重新检查"));
    note.error = Some("请求太频繁，稍等几秒再试".into());
    note.sanitize();
    assert_eq!(note.error.as_deref(), Some("请求太频繁，稍等几秒再试"));
}

#[test]
fn test_explicit_auxiliary_clear_updates_only_that_role_and_consumes_intent() {
    let (mut sys, _) = super::super::state_tests::databases();
    let mut config = config(30);
    let light = Role { provider_id: 30, code: "small".into(), verified: true };
    let review = Role { provider_id: 30, code: "audit".into(), verified: true };
    let mut saved = role_settings(Some(&light), Some(&review), false, false, false);
    saved.insert("default_decision_model".into(), "decision%%31".into());
    saved.insert("title_prompt".into(), "custom".into());
    crate::api::system_api::persist_feature_settings(&mut sys, "conversation_summary", saved, true)
        .unwrap();

    config.chat.code = "another-chat".into();
    config.review = None;
    config.clear_review = true;
    config.notes.insert("review".into(), RoleNote { reason: Some("手动选择".into()), error: None, choose_protocol: false });
    let checkpoint = serde_json::to_string(&config).unwrap();
    let mut recovered: Config = serde_json::from_str(&checkpoint).unwrap();
    assert!(recovered.clear_review);
    let patch = role_settings(None, None, recovered.clear_light, recovered.clear_review, false);
    assert_eq!(patch.len(), 2);
    crate::api::system_api::persist_feature_settings(&mut sys, "conversation_summary", patch, true)
        .unwrap();
    let value = |key| sys.get_feature_config("conversation_summary", key).unwrap().unwrap().value;
    assert_eq!(value("default_light_model"), "small%%30");
    assert_eq!(value("auto_review_model"), "");
    assert_eq!(value("auto_review_provider_id"), "");
    assert_eq!(value("default_decision_model"), "decision%%31");
    assert_eq!(value("title_prompt"), "custom");
    finish_role_clear(&mut recovered);
    assert!(recovered.review.is_none());
    assert!(!recovered.notes.contains_key("review"));
    assert!(!serde_json::to_value(recovered)
        .unwrap()
        .as_object()
        .unwrap()
        .contains_key("clearReview"));
}

#[test]
fn test_omitted_and_unverified_auxiliary_roles_do_not_clear_existing_settings() {
    let failed = Role { provider_id: 20, code: "unchecked".into(), verified: false };
    assert!(role_settings(None, Some(&failed), false, false, false).is_empty());
    assert!(validate_clear(None, true, Some(&failed), false).is_ok());
    let changes = role_settings(None, Some(&failed), true, false, false);
    assert_eq!(changes, HashMap::from([("default_light_model".into(), String::new())]));
    assert!(validate_clear(None, false, Some(&failed), true).is_err());
}

#[test]
fn test_role_only_recovery_clears_review_even_after_main_provider_and_workspace_changed() {
    let (mut sys, llm) = super::super::state_tests::databases();
    let mut c = config(7654321); // Main provider was removed after first-run configuration.
    c.workspace_path = Some("no longer a valid absolute directory".into());
    c.review = None;
    c.clear_review = true;
    let pending = Draft {
        config: Some(c),
        pending_role_change: Some(PendingRoleChange { light: false, review: true }),
        ..Draft::default()
    };
    put(&sys.conn, "onboarding_step", "source_ready").unwrap();
    save_draft(&sys.conn, &pending).unwrap();
    crate::api::system_api::persist_feature_settings(
        &mut sys,
        "experimental",
        HashMap::from([("default_home_window".into(), "ask".into())]),
        true,
    )
    .unwrap();
    let mut recovered = draft(&sys).unwrap();
    let mut c = recovered.config.clone().unwrap();
    let changes = prepare_role_update(&sys, &llm, &mut recovered, &mut c).unwrap();
    assert_eq!(
        changes,
        HashMap::from([
            ("auto_review_model".into(), String::new()),
            ("auto_review_provider_id".into(), String::new())
        ])
    );
    crate::api::system_api::persist_feature_settings(
        &mut sys,
        "conversation_summary",
        changes,
        true,
    )
    .unwrap();
    assert_eq!(experiment(&sys, "default_home_window").unwrap(), "ask");
    assert!(experiment(&sys, "butler_main_workspace_path").unwrap().is_empty());
    assert!(llm.get_llm_provider(7654321).is_err());
}

#[test]
fn test_clear_unverified_except_removes_abandoned_drafts_but_keeps_referenced_unverified_role() {
    let (sys, llm) = super::super::state_tests::databases();
    let abandoned = llm.add_llm_provider("Abandoned", "deepseek", "", false, false).unwrap();
    llm.mark_onboarding_draft(abandoned).unwrap();
    let failed_aux = llm.add_llm_provider("Aux", "deepseek", "", false, false).unwrap();
    llm.mark_onboarding_draft(failed_aux).unwrap();
    let mut d = Draft {
        providers: vec![
            DraftProvider { provider_id: abandoned, owned: true },
            DraftProvider { provider_id: failed_aux, owned: true },
        ],
        ..Default::default()
    };
    let mut cfg = config(30);
    cfg.light = Some(Role { provider_id: failed_aux, code: "deepseek-flash".into(), verified: false });
    let keep = cfg.referenced_provider_ids();
    assert!(clear_unverified_except(&sys, &llm, &mut d, &keep).unwrap());
    assert!(llm.get_llm_provider(abandoned).is_err());
    assert!(llm.get_llm_provider(failed_aux).is_ok());
    assert_eq!(d.providers.len(), 1);
    assert_eq!(d.providers[0].provider_id, failed_aux);
}

#[test]
fn test_onboarding_quick_assistant_options_preserve_sampling_and_survive_replay() {
    let db = AssistantDatabase {conn: Connection::open_in_memory().unwrap(), mcp_conn: Connection::open_in_memory().unwrap()};
    db.create_tables().unwrap();
    for limit in [32000,4096,4096] {
        apply_chat_defaults(&db, 1, limit).unwrap();
        let values = crate::api::assistant_api::management::config_value;
        assert_eq!(values(&db,1,"max_tokens").unwrap(), Some(limit.to_string()));
        for (key, value) in [("stream","true"),("mcp_loading_mode_explicit","true"),("dynamic_mcp_loading_enabled","true"),("temperature","0.75"),("tool_review_mode","auto_model")] {
            assert_eq!(values(&db,1,key).unwrap().as_deref(), Some(value));
        }
    }
}

#[test]
fn test_lie_flat_deepseek_saves_full_list_and_switches_only_in_this_mode() {
    for lie_flat in [true, false] {
        let (sys, llm) = super::super::state_tests::databases();
        put(&sys.conn, "onboarding_path", if lie_flat { "lie_flat" } else { "expert" }).unwrap();
        let mut c = config(30);
        c.models[0].codes.extend((1..=16).map(|i| format!("model-{i}")));
        let mut d = Draft::default();
        for _ in 0..2 { prepare_providers(&sys, &llm, &mut d, &mut c, None).unwrap(); }
        assert_eq!(llm.get_llm_models("30".into()).unwrap().len(), if lie_flat { 18 } else { 10 });
        let configs = llm.get_llm_provider_config(30).unwrap();
        for key in ["deepseek_web_enabled", "deepseek_show_balance_in_chat"] {
            assert_eq!(configs.iter().any(|c| c.name == key && c.value == "true"), lie_flat);
        }
    }
}

#[test]
fn test_lie_flat_review_is_the_inherited_decision_model() {
    let review = Role { provider_id: 30, code: "deepseek-flash".into(), verified: true };
    let settings = role_settings(None, Some(&review), false, false, true);
    assert_eq!(settings["default_decision_model"], "deepseek-flash%%30");
    assert_eq!(settings["auto_review_model"], "");
    assert_eq!(settings["auto_review_provider_id"], "");
    let cleared = role_settings(None, None, false, true, true);
    assert_eq!(cleared["default_decision_model"], "");
}

#[test]
fn test_lie_flat_quick_appearance_preserves_a_customized_assistant_on_replay() {
    let (sys, _) = super::super::state_tests::databases();
    let db = AssistantDatabase {conn: Connection::open_in_memory().unwrap(), mcp_conn: Connection::open_in_memory().unwrap()};
    db.create_tables().unwrap();
    let saved = || db.conn.query_row("SELECT value FROM assistant_appearance WHERE assistant_id=1", [], |r| r.get::<_, String>(0)).unwrap();
    apply_quick_appearance(&sys, &db).unwrap();
    let logo: serde_json::Value = serde_json::from_str(&saved()).unwrap();
    assert_eq!(logo["shape"], "aipp");
    assert_eq!(logo["accessories"], serde_json::json!({"eyes":null,"head":null}));
    db.conn.execute("UPDATE assistant_appearance SET value='custom' WHERE assistant_id=1", []).unwrap();
    apply_quick_appearance(&sys, &db).unwrap();
    assert_eq!(saved(), "custom");
}
