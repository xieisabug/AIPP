use super::*;
use crate::api::ai::conversation::{build_chat_request_from_messages, ToolCallStrategy};
use crate::api::genai_client::create_client_with_config;
use crate::db::conversation_db::MessageRepository;
use crate::db::llm_db::LLMProviderConfig;
use crate::db::system_db::FeatureConfig;
use crate::errors::AppError;
use std::collections::HashMap;
use std::sync::Mutex;

impl persistence::CheckpointStore for Mutex<MessageRepository> {
    fn save(&self, conversation: i64, header: &str, content: &str) -> Result<String, AppError> {
        Ok(self.lock().unwrap().get_or_create_checkpoint(conversation, header, content)?)
    }
}

fn msg(role: &str, text: impl Into<String>) -> MessageTuple {
    (role.into(), text.into(), vec![])
}

async fn request(client: &Client, model: &str, label: &str, messages: &[MessageTuple]) -> String {
    let request = build_chat_request_from_messages(messages, ToolCallStrategy::Native, None).chat_request;
    let response = client.exec_chat(model, request, Some(&genai::chat::ChatOptions::default().with_max_tokens(1024)))
        .await.expect("live cache request failed");
    let input = response.usage.prompt_tokens.expect("provider did not report input tokens");
    let cached = response.usage.prompt_tokens_details.as_ref().and_then(|v| v.cached_tokens);
    println!("{label}: input={input}, cached={cached:?}");
    response.first_text().expect("provider did not return an answer").to_owned()
}

#[tokio::test]
#[ignore = "opt-in paid model probe; requires AIPP_CACHE_PROBE_ENDPOINT, MODEL and KEY"]
async fn test_live_butler_checkpoint_cache() {
    let config = |name: &str, value| LLMProviderConfig {
        id: 0, name: name.into(), llm_provider_id: 1, value, append_location: String::new(), is_addition: false,
    };
    let model = std::env::var("AIPP_CACHE_PROBE_MODEL").unwrap();
    let endpoint = std::env::var("AIPP_CACHE_PROBE_ENDPOINT").unwrap();
    let key = std::env::var("AIPP_CACHE_PROBE_KEY").unwrap();
    let headers = FeatureConfig { id: None, feature_code: "network_config".into(), key: "custom_headers".into(),
        value: r#"{"User-Agent":"AIPP-Butler-Cache-Verification/1.0"}"#.into(), data_type: "string".into(), description: None };
    let features = HashMap::from([(headers.feature_code.clone(), HashMap::from([(headers.key.clone(), headers)]))]);
    let client = create_client_with_config(&[config("endpoint", endpoint), config("api_key", key)],
        &model, "openai_api", None, None, false, Some(150), false, &features).unwrap();
    let db = rusqlite::Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE message(id INTEGER PRIMARY KEY, conversation_id INTEGER, message_type TEXT,
        content TEXT, created_time TEXT, token_count INTEGER, input_token_count INTEGER, output_token_count INTEGER)").unwrap();
    let store = Mutex::new(MessageRepository::new(db));
    let sample = |tag: &str, count: usize| (0..count)
        .map(|i| format!("{tag} entry {i} belongs to the sample team and needs review.\n"))
        .collect::<String>();
    let mut history = vec![
        msg("system", format!("Probe {}. Reply briefly.\n{}", uuid::Uuid::new_v4(), sample("rules",100))),
        msg("user", format!("The acceptance word is cedar. Keep this decision.\n{}", sample("earlier",140))),
        msg("response","Recorded cedar."),
        msg("user",format!("{}\nReply OK.", sample("recent",55))),
    ];
    let first = request(&client,&model,"cold",&history).await;
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    history.extend([msg("response",first),msg("user","Reply OK again.")]);
    let warm = request(&client,&model,"before",&history).await;
    history.extend([msg("response",warm),msg("user","What was the acceptance word?")]);
    let budget = ContextBudget { context_window_size: estimate_total(&history,&[]), output_reserve: 0, tool_call_reserve: 0,
        tail_ratio: 0.25, compaction_threshold: 0.90, enabled: true };
    let result = fit_to_budget_with_compaction(history.clone(), &budget, &[], CompactionContext {
        client: &client, model_name: &model, conversation_id: 1, checkpoint_store: &store, is_butler: true,
        message_ids: (1..=history.len() as i64).collect(),
    }).await.unwrap();
    assert!(result.compacted);
    let answer = request(&client,&model,"checkpoint",&result.messages).await;
    assert!(answer.to_lowercase().contains("cedar"), "{answer}");
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    let mut next = result.messages;
    next.extend([msg("response", answer),msg("user","Repeat that word only.")]);
    let continued = request(&client,&model,"after_append",&next).await;
    assert!(continued.to_lowercase().contains("cedar"));
}
