use super::*;

fn field(key: &str, name: &str) -> InputField {
    InputField { key: key.into(), name: name.into(), description: String::new() }
}

fn queue(request_id: &str, conversation_id: Option<i64>, created_at: i64) -> oneshot::Receiver<Answer> {
    let (sender, receiver) = oneshot::channel();
    let request = VaultInputRequest {
        request_id: request_id.into(), conversation_id, message_id: None, assistant_name: None,
        purpose: "测试".into(), fields: vec![field("id_card", "身份证号")], created_at,
    };
    PENDING.lock().unwrap().insert(request_id.into(), Pending { request, sender: Some(sender) });
    receiver
}

#[test]
fn test_fields_are_validated_before_asking() {
    assert!(validate_fields(&[field("id_card", "身份证号"), field("phone", "手机号")]).is_ok());
    assert_eq!(validate_fields(&[]).unwrap_err().code, "invalid_request");
    let six: Vec<InputField> = (0..6).map(|i| field(&format!("k{i}x"), "名称")).collect();
    assert_eq!(validate_fields(&six).unwrap_err().code, "invalid_request");
    assert!(validate_fields(&[field("ID", "身份证号")]).is_err(), "keys follow the vault key rule");
    assert!(validate_fields(&[field("id_card", "  ")]).is_err(), "the card needs a label");
    let error = validate_fields(&[field("id_card", "a"), field("id_card", "b")]).unwrap_err();
    assert!(error.message.contains("重复"));
}

#[tokio::test]
async fn test_decline_answers_the_waiting_call_once() {
    let mut receiver = queue("decline-1", Some(7), 1);
    vault_decline_input("decline-1".into()).await.unwrap();
    assert!(matches!(receiver.try_recv(), Ok(Answer::Declined)));
    // The card cannot be answered twice; the waiting call removes the entry when it ends.
    assert!(vault_decline_input("decline-1".into()).await.is_err());
    PENDING.lock().unwrap().remove("decline-1");
    assert!(vault_decline_input("unknown".into()).await.is_err());
}

#[tokio::test]
async fn test_listing_filters_by_conversation_and_never_holds_values() {
    let _a = queue("list-a", Some(901), 2);
    let _b = queue("list-b", Some(902), 1);
    let only = vault_list_input_requests(Some(901)).await.unwrap();
    assert_eq!(only.iter().map(|r| r.request_id.as_str()).collect::<Vec<_>>(), vec!["list-a"]);
    let all: Vec<String> = vault_list_input_requests(None).await.unwrap().into_iter()
        .filter(|r| r.request_id.starts_with("list-")).map(|r| r.request_id).collect();
    assert_eq!(all, vec!["list-b", "list-a"], "oldest first");
    assert!(pending_conversation_ids().contains(&902));
    let shown = serde_json::to_value(&only[0]).unwrap();
    assert_eq!(shown["fields"][0], serde_json::json!({"key": "id_card", "name": "身份证号", "description": ""}));
    PENDING.lock().unwrap().retain(|id, _| !id.starts_with("list-"));
}
