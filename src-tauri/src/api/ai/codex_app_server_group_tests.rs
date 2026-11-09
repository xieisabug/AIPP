use super::*;

#[test]
fn test_group_steer_keeps_native_turn_and_exact_payload() {
    let input=crate::db::butler_group_native::NativeInput {id:19,payload:"immutable\n群输入".into()};
    let params=group_steer_params("thread-a","turn-a",&input);
    assert_eq!(params["threadId"],"thread-a");
    assert_eq!(params["expectedTurnId"],"turn-a");
    assert_eq!(params["input"][0]["text"],input.payload);
    assert_eq!(params["clientUserMessageId"],"aipp-group-input-19");
}

#[test]
fn test_group_steer_stops_when_final_or_unclassified_answer_begins() {
    let mut commentary=std::collections::HashSet::new(); let mut final_started=false;
    note_group_output_phase("item/started",&json!({"item":{"type":"agentMessage","id":"progress","phase":"commentary"}}),&mut commentary,&mut final_started);
    note_group_output_phase("item/agentMessage/delta",&json!({"itemId":"progress","delta":"working"}),&mut commentary,&mut final_started);
    assert!(!final_started);
    note_group_output_phase("item/started",&json!({"item":{"type":"agentMessage","id":"answer","phase":"final_answer"}}),&mut commentary,&mut final_started);
    assert!(final_started);
    final_started=false;
    note_group_output_phase("item/agentMessage/delta",&json!({"itemId":"legacy","delta":"answer"}),&mut commentary,&mut final_started);
    assert!(final_started);
}

#[cfg(windows)]
struct NoDesktop;
#[cfg(windows)]
#[async_trait::async_trait]
impl CodexServerRequests for NoDesktop {
    async fn handle(&self,_stdin:&mut tokio::process::ChildStdin,_frame:&Value)->Result<bool,String> {Ok(false)}
}

/// Real child pipes drive the production wire function. Inline script only:
/// no database, fixture files, live service, login or model invocation.
#[cfg(windows)]
async fn wire_case(response:&str)->(Result<Result<(),String>,String>,VecDeque<Value>) {
    let script=format!(r#"
$ErrorActionPreference='Stop'
$frame=[Console]::In.ReadLine() | ConvertFrom-Json
if ($frame.method -ne 'turn/steer' -or $frame.params.expectedTurnId -ne 'turn-a' -or $frame.params.threadId -ne 'thread-a' -or $frame.params.input[0].text -ne 'frozen-input') {{ exit 41 }}
[Console]::Out.WriteLine('{{"jsonrpc":"2.0","method":"item/started","params":{{"threadId":"thread-a","turnId":"turn-a","item":{{"id":"work","type":"reasoning"}}}}}}')
[Console]::Out.WriteLine('{}')
[Console]::Out.Flush()
if ([Console]::In.ReadLine()) {{ exit 42 }}
"#,response.replace('\'',"''"));
    let mut child=tokio::process::Command::new("pwsh").args(["-NoLogo","-NoProfile","-NonInteractive","-Command",&script])
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
        .creation_flags(0x08000000).kill_on_drop(true).spawn().unwrap();
    let mut stdin=child.stdin.take().unwrap();
    let mut lines=BufReader::new(child.stdout.take().unwrap()).lines();
    let mut pending=VecDeque::new();
    let input=crate::db::butler_group_native::NativeInput {id:19,payload:"frozen-input".into()};
    let result=tokio::time::timeout(std::time::Duration::from_secs(10),send_group_steer_wire(&NoDesktop,1,&mut stdin,&mut lines,&mut pending,7,"thread-a","turn-a",&input)).await.unwrap();
    drop(stdin);
    assert!(tokio::time::timeout(std::time::Duration::from_secs(5),child.wait()).await.unwrap().unwrap().success());
    (result,pending)
}

#[cfg(windows)]
#[tokio::test]
async fn test_group_steer_wire_preserves_notifications_before_ack() {
    let (result,pending)=wire_case(r#"{"jsonrpc":"2.0","id":7,"result":{"turnId":"turn-a"}}"#).await;
    result.unwrap().unwrap();
    assert_eq!(pending.len(),1);
    assert_eq!(pending[0]["params"]["item"]["id"],"work");
}

#[cfg(windows)]
#[tokio::test]
async fn test_group_steer_wire_rejects_other_turn_without_retry() {
    let (result,_)=wire_case(r#"{"jsonrpc":"2.0","id":7,"result":{"turnId":"turn-b"}}"#).await;
    assert!(result.unwrap_err().contains("expected=turn-a"));
    let (result,_)=wire_case(r#"{"jsonrpc":"2.0","id":7,"error":{"code":-32600,"message":"active turn mismatch"}}"#).await;
    assert!(result.unwrap().unwrap_err().contains("active turn mismatch"));
    let (result,_)=wire_case(r#"{"jsonrpc":"2.0","id":7,"error":null}"#).await;
    assert!(result.unwrap_err().contains("不完整"));
    let (result,_)=wire_case(r#"{"jsonrpc":"2.0","id":7,"result":{"turnId":"turn-a"},"error":{"code":-32600,"message":"conflict"}}"#).await;
    assert!(result.unwrap_err().contains("同时包含"));
}
