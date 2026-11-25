// Prompt contracts: these tests check what the prompt says, not that a live model follows it.
use super::{delivery_prompt, DeliveryTools};
use crate::api::assistant_api::{MCPServerWithTools, MCPToolInfo};

fn server(command: &str, enabled: bool, tools: &[(&str, bool)]) -> MCPServerWithTools {
    MCPServerWithTools {
        id: 1,
        name: command.into(),
        summary: String::new(),
        command: Some(command.into()),
        is_enabled: enabled,
        tools: tools
            .iter()
            .map(|(name, is_enabled)| MCPToolInfo {
                id: 1,
                name: (*name).into(),
                description: String::new(),
                is_enabled: *is_enabled,
                is_auto_run: false,
                parameters: "{}".into(),
            })
            .collect(),
    }
}

fn all_tools() -> DeliveryTools {
    DeliveryTools {
        ask_user_question: true,
        present_choices: true,
        preview_file: true,
        preview_code: true,
        artifact: true,
        artifact_screenshot: true,
    }
}

const FILE_FORMATS: [&str; 3] = ["Markdown 文件（.md）", "Word 文档（.docx）", "Excel 工作簿（.xlsx）"];

#[test]
fn detects_only_enabled_builtin_tools() {
    let servers = vec![
        server(
            "aipp:ui_interaction",
            true,
            &[("ask_user_question", true), ("preview_file", true), ("preview_code", false)],
        ),
        server("aipp:artifact", true, &[("get_artifact_workspace", true), ("show_artifact", true)]),
        // 同名工具出现在第三方服务里不算内置能力。
        server("npx some-mcp", true, &[("preview_code", true), ("capture_artifact_screenshot", true)]),
    ];
    assert_eq!(
        DeliveryTools::from_servers(&servers),
        DeliveryTools {
            ask_user_question: true,
            present_choices: false,
            preview_file: true,
            preview_code: false,
            artifact: true,
            artifact_screenshot: false,
        }
    );

    let disabled = vec![
        server("aipp:ui_interaction", false, &[("preview_code", true)]),
        server("aipp:artifact", true, &[("show_artifact", true)]),
    ];
    let tools = DeliveryTools::from_servers(&disabled);
    assert!(!tools.preview_code, "整个服务被关闭时不算可用");
    assert!(!tools.artifact, "没有 get_artifact_workspace 就拿不到工作区路径");
}

#[test]
fn full_toolset_covers_demo_input_tools_diagrams_and_channels() {
    let prompt = delivery_prompt(all_tools(), false);
    for phrase in FILE_FORMATS.into_iter().chain([
        "讲解时可以边说边演示",
        "`interaction_mode=submit_once`",
        "超出 `ask_user_question` 选项能表达的范围",
        "Mermaid（.mmd）或 draw.io（.drawio",
        "用户要反复使用的小工具",
        "`db_id`",
        "AIPP Artifact 相关的 Skill",
        "`capture_artifact_screenshot`",
        "Word、Excel、PPT、PDF 或图片可以用 `preview_file`",
        "不必每次复读系统操作提示",
        "`<external_channel_input>`",
        "看不到 `preview_code` 演示和 Artifact，",
    ]) {
        assert!(prompt.contains(phrase), "缺少：{phrase}\n{prompt}");
    }
    assert!(!prompt.contains("包含多个文件"), "预览只加载入口文件，不能再引导写多文件 Artifact");
}

#[test]
fn missing_tools_are_never_mentioned() {
    let prompt = delivery_prompt(DeliveryTools::default(), false);
    for phrase in FILE_FORMATS {
        assert!(prompt.contains(phrase), "文件交付不依赖聊天展示工具：{phrase}");
    }
    for tool in ["preview_code", "preview_file", "show_artifact", "capture_artifact_screenshot", "Artifact", "external_channel_input"] {
        assert!(!prompt.contains(tool), "未注入的能力不该出现：{tool}\n{prompt}");
    }

    let without_ask = delivery_prompt(DeliveryTools { preview_code: true, ..Default::default() }, false);
    assert!(without_ask.contains("`interaction_mode=submit_once`"));
    assert!(!without_ask.contains("ask_user_question"));
    assert!(without_ask.contains("看不到 `preview_code` 演示，"));
}

#[test]
fn unattended_runs_keep_only_file_delivery() {
    let prompt = delivery_prompt(all_tools(), true);
    for phrase in FILE_FORMATS {
        assert!(prompt.contains(phrase));
    }
    for tool in ["preview_code", "preview_file", "show_artifact", "submit_once", "Artifact"] {
        assert!(!prompt.contains(tool), "定时任务的运行会话对用户不可见：{tool}");
    }
}
