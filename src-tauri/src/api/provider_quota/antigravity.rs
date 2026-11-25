use super::QuotaWindow;
use crate::api::ai::{
    acp::apply_network_proxy_to_env_vars,
    agent_driver::{failure::redact_agent_diagnostic, native_agents::env},
};
use crate::utils::background_process::background_command;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::HashMap, process::Stdio, time::Duration};

#[derive(Deserialize)]
struct ModelGroup {
    name: String,
    buckets: Vec<Bucket>,
}

#[derive(Deserialize)]
struct Bucket {
    window: String,
    remaining_fraction: Option<f64>,
    remaining_amount: Option<f64>,
    reset_time: Option<DateTime<Utc>>,
}

pub(super) fn windows(report: Value) -> Result<Vec<QuotaWindow>, String> {
    if report["status"] != "SUCCESS" {
        let error = report["error"].as_str().unwrap_or("CLI 未提供错误详情");
        return Err(format!("agy /usage 查询失败：{}", redact_agent_diagnostic(error, &[])));
    }
    if report["command"]["name"] != "usage" || report["num_turns"] != 0 {
        return Err("agy 未返回内置 /usage 命令的只读结果".into());
    }
    let groups = Vec::<ModelGroup>::deserialize(&report["command"]["data"]["groups"])
        .map_err(|e| format!("agy /usage 模型组或额度窗口格式无效：{e}"))?;
    let mut windows = Vec::new();
    for group in groups {
        for bucket in group.buckets {
            let label = match bucket.window.as_str() {
                "5h" => "5 小时".into(),
                "weekly" => "每周".into(),
                _ => bucket.window,
            };
            windows.push(QuotaWindow {
                group: Some(group.name.clone()),
                label,
                remaining_percent: bucket.remaining_fraction.map(|v| (v * 100.0).clamp(0.0, 100.0)),
                remaining_amount: bucket.remaining_amount,
                resets_at: bucket.reset_time,
            });
        }
    }
    if windows.is_empty() {
        return Err("本机 Antigravity CLI 未返回额度窗口，请检查 agy 的登录状态和套餐".into());
    }
    Ok(windows)
}

pub(super) async fn local_quota(proxy: Option<&str>) -> Result<Vec<QuotaWindow>, String> {
    // This command belongs to the machine's CLI, regardless of the provider's ACP runtime or login.
    let directory = tempfile::tempdir().map_err(|e| format!("无法为 agy /usage 创建临时工作目录：{e}"))?;
    let cwd = directory.path().to_owned();
    let executable = tokio::task::spawn_blocking(move || {
        let name = if cfg!(windows) { "agy.exe" } else { "agy" };
        env::cli_candidates("antigravity", false, name, &HashMap::new(), &cwd).into_iter().next()
    }).await.map_err(|e| format!("查找本机 Antigravity CLI 失败：{e}"))?
        .ok_or("未找到本机 Antigravity CLI，请安装官方 agy 并确认可在本机运行")?.0;
    // Older releases may interpret /usage as a model prompt.
    let version = env::program_version(&executable).await.map_err(|e| format!("检测 agy 版本失败：{e}"))?;
    let version = semver::Version::parse(&version).map_err(|e| format!("无法识别 agy 版本：{e}"))?;
    if version < semver::Version::new(1, 1, 11) {
        return Err(format!("本机 agy {version} 不支持只读额度查询，请更新至 1.1.11 或更高版本"));
    }
    let mut cmd = tokio::process::Command::from(background_command(executable));
    cmd.args(["--settings", r#"{"disableSlashCommands":false}"#, "-p", "/usage", "--output-format", "json"])
        .current_dir(directory.path()).stdin(Stdio::null()).kill_on_drop(true);
    if let Some(proxy) = proxy {
        let mut env = HashMap::new();
        apply_network_proxy_to_env_vars(&mut env, proxy);
        cmd.envs(env);
    }
    let output = tokio::time::timeout(Duration::from_secs(30), cmd.output()).await
        .map_err(|_| "本机 agy /usage 在 30 秒内未返回")?
        .map_err(|e| format!("运行本机 agy /usage 失败：{e}"))?;
    let report = serde_json::from_slice::<Value>(&output.stdout);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = report.as_ref().ok().and_then(|v| v["error"].as_str())
            .filter(|s| !s.is_empty()).unwrap_or_else(|| stderr.trim());
        let reason = if reason.is_empty() { "CLI 未提供错误详情" } else { reason };
        return Err(format!("agy /usage 退出状态 {}：{}", output.status, redact_agent_diagnostic(reason, &[])));
    }
    windows(report.map_err(|e| format!("agy /usage 未返回有效 JSON：{e}"))?)
}
