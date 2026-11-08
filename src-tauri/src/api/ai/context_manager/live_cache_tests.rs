use super::*;
use crate::api::ai::conversation::{build_chat_request_from_messages, ToolCallStrategy};
use crate::db::conversation_db::MessageRepository;
use crate::errors::AppError;
use std::sync::Mutex;

impl persistence::CheckpointStore for Mutex<MessageRepository> {
    fn ensure_execution(&self, _: i64, _: Option<i64>) -> Result<(), AppError> { Ok(()) }
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
    let (client, model) = crate::api::live_test_client::configured_client();
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
        source_message_id: None, cancel_token: None,
        client: &client, model_name: &model, conversation_id: 1, checkpoint_store: &store, summary_role: super::summarizer::SummaryRole::ButlerChief, require_complete_summary: true,
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
