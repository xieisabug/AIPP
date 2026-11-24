use super::*;

fn adapter(reply: &str) -> Command {
    #[cfg(windows)]
    {
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command",
            &format!("$request = [Console]::ReadLine() | ConvertFrom-Json; if ($request.method -eq 'initialize') {{ [Console]::WriteLine('{reply}') }}")]);
        command
    }
    #[cfg(unix)]
    {
        let mut command = Command::new("sh");
        command.args(["-c", &format!("read -r frame; case \"$frame\" in *initialize*) echo '{reply}';; esac")]);
        command
    }
}

#[tokio::test]
async fn test_acp_adapter_reports_running_version_and_rejects_missing_initialize() {
    let version = initialize(adapter(r#"{"jsonrpc":"2.0","id":1,"result":{"agentInfo":{"version":"0.13.0"}}}"#)).await.unwrap();
    assert_eq!(version, "0.13.0");
    let failed = initialize(adapter(r#"{"jsonrpc":"2.0","id":1,"error":{"message":"adapter failed"}}"#)).await.unwrap_err();
    assert!(failed.contains("adapter failed"));
    let empty = initialize(adapter(r#"{"id":1,"result":{}}"#)).await.unwrap_err();
    assert!(empty.contains("agentInfo.version"));
}
