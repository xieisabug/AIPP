use super::*;

fn request() -> Request {
    Request {
        task_id: 6,
        task_name: "日报推送".into(),
        task_prompt: "每天把日报发到飞书群「研发周会」".into(),
        assistant_name: "助理".into(),
        run_id: "run-1".into(),
        server_name: "飞书".into(),
        tool_name: "send_message".into(),
        parameters: "{\"chat\":\"研发周会\"}".into(),
        review_reason: "向外部发送消息，需要确认收件方".into(),
        progress: String::new(),
    }
}

#[test]
fn test_request_message_carries_intent_call_and_reason() {
    let message = request_message("sched-1", &request());
    for expected in ["request_id=\"sched-1\"", "日报推送", "每天把日报发到飞书群", "飞书 / send_message", "需要确认收件方", "还没有输出"] {
        assert!(message.contains(expected), "missing {expected}: {message}");
    }
    let prompt = request_prompt("sched-1", "日报推送");
    assert!(prompt.contains("decide") && prompt.contains("sched-1") && prompt.contains("不要向用户提问"));
}

#[test]
fn test_request_message_truncates_large_parameters() {
    let big = Request { parameters: "x".repeat(10_000), ..request() };
    let message = request_message("sched-2", &big);
    assert!(message.contains("已截断"));
    assert!(message.chars().count() < 6_000);
}

#[tokio::test]
async fn test_decide_delivers_once_and_rejects_unknown_requests() {
    let (sender, receiver) = oneshot::channel();
    pending().lock().unwrap().insert("sched-test".into(), Pending { sender, task_name: "日报推送".into() });
    assert!(decide("sched-test", true, "目标是用户指定的群").unwrap().contains("允许"));
    assert_eq!(receiver.await.unwrap(), Decision::Allow("目标是用户指定的群".into()));
    assert!(decide("sched-test", false, "").unwrap_err().contains("不存在或已结束"));
}

#[tokio::test]
async fn test_decide_reports_when_run_already_ended() {
    let (sender, receiver) = oneshot::channel::<Decision>();
    drop(receiver);
    pending().lock().unwrap().insert("sched-gone".into(), Pending { sender, task_name: "日报推送".into() });
    assert!(decide("sched-gone", false, "不在范围内").unwrap_err().contains("运行已结束"));
}
