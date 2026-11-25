//! 管家模式（总管家、联系人、独立任务、群成员）与管家定时任务共用的「交付与展示」约定。
//! 按本轮实际注入的工具拼接：没有注入的工具不会出现在提示词里，避免让模型去调用不存在的工具。
use crate::api::assistant_api::MCPServerWithTools;
use crate::mcp::builtin_mcp::builtin_command_id;

/// 交付相关的内置工具在本轮是否可用。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DeliveryTools {
    pub ask_user_question: bool,
    pub present_choices: bool,
    pub preview_file: bool,
    pub preview_code: bool,
    /// `get_artifact_workspace` 与 `show_artifact` 都可用才算能发布 Artifact。
    pub artifact: bool,
    pub artifact_screenshot: bool,
}

impl DeliveryTools {
    pub(crate) fn from_servers(servers: &[MCPServerWithTools]) -> Self {
        let has = |builtin_id: &str, tool: &str| {
            servers.iter().any(|server| {
                server.is_enabled
                    && server.command.as_deref().and_then(builtin_command_id).as_deref()
                        == Some(builtin_id)
                    && server.tools.iter().any(|t| t.is_enabled && t.name == tool)
            })
        };
        let artifact =
            has("artifact", "get_artifact_workspace") && has("artifact", "show_artifact");
        Self {
            ask_user_question: has("ui_interaction", "ask_user_question"),
            present_choices: has("ui_interaction", "present_choices"),
            preview_file: has("ui_interaction", "preview_file"),
            preview_code: has("ui_interaction", "preview_code"),
            artifact,
            artifact_screenshot: artifact && has("artifact", "capture_artifact_screenshot"),
        }
    }
}

/// 生成「交付与展示」约定。
///
/// `unattended` 用于定时任务：用户不在场、运行会话对用户不可见，只投递最终回复，
/// 所以只保留文件交付，不提聊天内展示、演示和交互收集。
pub(crate) fn delivery_prompt(tools: DeliveryTools, unattended: bool) -> String {
    let tools = if unattended { DeliveryTools::default() } else { tools };
    let mut lines: Vec<String> = vec![
        "## 交付与展示".into(),
        "用户指定的格式、交付渠道和已有文件优先。简短问答、进度沟通直接在聊天中回答。".into(),
    ];

    if tools.present_choices {
        lines.push("需要用户在 2–6 份实际文稿、图片或界面版本中拍板时，用 `present_choices` 发送候选并等待选择。已有文件或完整 Artifact 工程可以直接引用。飞书来源会收到提示，用户回到 AIPP 桌面的原会话作答。".into());
    }
    if tools.preview_code {
        lines.push("讲解时可以边说边演示：交互或图示明显比文字好懂时（算法步骤、参数变化、布局效果、数据对比），用 `preview_code` 在聊天里放一个小演示，结论仍用文字说清；一两句话能说明白的不做演示。".into());
        let beyond_ask = if tools.ask_user_question {
            "，超出 `ask_user_question` 选项能表达的范围"
        } else {
            ""
        };
        lines.push(format!("需要用户调参数、排序、圈选，或一次填写多项内容{beyond_ask}时，用 `preview_code` 的 `interaction_mode=submit_once` 做一个小表单，用户提交的内容会作为工具结果返回。"));
    }

    lines.push("用户要一份可带走、编辑或复用的成果时，按用途选一种主要载体：".into());
    let markdown_preview =
        if tools.preview_file { "；需要在聊天里阅读时，用 `preview_file` 展示该文件" } else { "" };
    lines.push(format!("- 文章、方案、说明和纯文字报告写成 Markdown 文件（.md）{markdown_preview}。"));
    lines.push("- 以图文叙述为主的报告或教程生成 Word 文档（.docx），把实际图片嵌入对应段落；单独要一张图时直接交付图片。".into());
    lines.push("- 以表格数据、计算或筛选为主时生成 Excel 工作簿（.xlsx），数值放入单元格，需要重算的结果写成公式。".into());
    if tools.artifact {
        lines.push("- 流程图、架构图、关系图写成 Mermaid（.mmd）或 draw.io（.drawio，用户可以继续编辑和导出）文件，用 `show_artifact` 发布。".into());
    }
    if tools.preview_code {
        lines.push("- 在聊天里看一次就够的演示或交互图表，用 `preview_code` 渲染 HTML。".into());
    }
    if tools.artifact {
        lines.push("- 用户要反复使用的小工具（记账、清单、计算器、需要保存数据或调用 AI 的工具），在 Artifact 工作区写成单个入口文件，用 `show_artifact` 发布；需要时绑定数据库（`db_id`）和助手（`assistant_id`），让工具能保存数据、调用 AI。具体写法可以先加载 AIPP Artifact 相关的 Skill（如有）。".into());
    }
    lines.push("- 仓库开发任务直接修改目标工程。".into());

    let mut check = String::from("生成文件时使用可用的 Skills 或工具，确认文件可打开、内容齐全");
    if tools.artifact_screenshot {
        check.push_str("；发布 Artifact 后用 `capture_artifact_screenshot` 截图检查渲染结果");
    }
    check.push('。');
    if tools.preview_file {
        check.push_str("生成的 Word、Excel、PPT、PDF 或图片可以用 `preview_file` 在聊天里展示（`url` 填本地路径）。");
    }
    check.push_str("最终回复给出结果和实际文件路径或链接");
    if tools.artifact {
        check.push_str("；发布了 Artifact 时提一下名称即可，不必每次复读系统操作提示");
    }
    check.push_str("。若所需格式生成失败，说明原因，和用户商量替代格式。");
    lines.push(check);

    let invisible = match (tools.preview_code, tools.artifact) {
        (true, true) => Some("`preview_code` 演示和 Artifact"),
        (true, false) => Some("`preview_code` 演示"),
        (false, true) => Some("Artifact"),
        (false, false) => None,
    };
    if let Some(invisible) = invisible {
        lines.push(format!(
            "消息带 `<external_channel_input>`（例如来自飞书）时，用户在那边看不到 {invisible}，只看得到文字和文件：改用文字结论或生成文件交付。"
        ));
    }

    lines.join("\n")
}

#[cfg(test)]
#[path = "butler_delivery_tests.rs"]
mod tests;
