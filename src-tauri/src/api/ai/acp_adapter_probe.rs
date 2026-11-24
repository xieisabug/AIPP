use std::{collections::HashMap, path::Path, process::{Command, Stdio}, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// Some claude-code-acp releases start an ACP server even with --version.
/// initialize returns its real version without creating a session or running a model.
pub(crate) async fn probe_version(
    path: &Path,
    environment: &HashMap<String, String>,
) -> Result<String, String> {
    let command = super::acp::build_acp_command("claude-code-acp", path, &[], environment);
    let diagnostics = super::agent_driver::failure::AgentDiagnosticBuffer::for_environment(environment);
    initialize(command).await
        .map(|version| diagnostics.sanitize(&version).chars().take(128).collect())
        .map_err(|e| format!("{} ACP initialize：{}", path.display(), diagnostics.sanitize(&e)))
}

async fn initialize(command: Command) -> Result<String, String> {
    let mut child = tokio::process::Command::from(command)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .kill_on_drop(true).spawn().map_err(|e| format!("启动失败：{e}"))?;
    let mut input = child.stdin.take().expect("piped stdin");
    let mut lines = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
    let mut errors = child.stderr.take().expect("piped stderr");
    let diagnostics = tokio::spawn(async move {
        let mut bytes = Vec::new();
        let _ = (&mut errors).take(4096).read_to_end(&mut bytes).await;
        let _ = tokio::io::copy(&mut errors, &mut tokio::io::sink()).await;
        String::from_utf8_lossy(&bytes).trim().to_owned()
    });
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let request = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":1,\"clientCapabilities\":{}}}\n";
        input.write_all(request).await.map_err(|e| format!("写入请求失败：{e}"))?;
        drop(input);
        while let Some(line) = lines.next_line().await.map_err(|e| format!("读取响应失败：{e}"))? {
            let frame: serde_json::Value = serde_json::from_str(&line)
                .map_err(|e| format!("响应不是有效 JSON：{e}"))?;
            if frame.get("id").and_then(|v| v.as_u64()) != Some(1) { continue; }
            if let Some(error) = frame.get("error") {
                return Err(format!("服务端返回错误：{error}"));
            }
            return frame.pointer("/result/agentInfo/version").and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty()).map(str::to_owned)
                .ok_or_else(|| "响应缺少 agentInfo.version".to_owned());
        }
        let status = child.wait().await.map_err(|e| format!("读取退出状态失败：{e}"))?;
        Err(format!("进程已结束（{status}），没有收到初始化响应"))
    }).await.unwrap_or_else(|_| Err("超过 10 秒未收到初始化响应".into()));
    let _ = child.kill().await;
    let stderr = tokio::time::timeout(Duration::from_millis(500), diagnostics).await
        .ok().and_then(Result::ok).unwrap_or_default();
    result.map_err(|message| if stderr.is_empty() { message } else { format!("{message}；{stderr}") })
}

#[cfg(test)]
#[path = "acp_adapter_probe_tests.rs"]
mod tests;
