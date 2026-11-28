import React from "react";
import type { AssistantFormConfig } from "@/types/forms";
import { claudePermissionModes } from "@/data/claudePermissionModes";

type ConfigGetter = (name: string, defaultValue: string) => string;
type ConfigSetter = (key: string, value: string | boolean, valueType: string) => void;

const REVIEW_TOOLTIP =
    "智能体请求执行操作时：人工审批会弹出确认；AIPP 模型自动审核先由「设置 → AI 与自动化」中的审核模型判断，安全的直接按「本次允许」执行（不会替你选「始终允许」），有风险或审核失败才弹出确认并说明原因。";

/**
 * Native permission controls and AIPP review settings for agent assistants.
 * Empty stored values mean the safe default (every action asks).
 */
export function buildNativeAgentApprovalFields(
    apiType: string,
    get: ConfigGetter,
    set: ConfigSetter,
    reviewModelConfigured: boolean,
    missingModelHint?: () => React.ReactNode,
    providerReviewMode?: "off" | "auto_model" | null,
): AssistantFormConfig[] {
    if (apiType === "cursor") {
        return [{
            key: "cursor_approval_notice",
            config: {
                type: "static" as const,
                label: "审批",
                value: "Cursor 默认开启 SDK 自动审核（Auto-review），可以在聊天中关闭。关闭后工具可直接执行，SDK 不提供逐项人工审批。",
            },
        }];
    }
    if (!["pi", "opencode", "kiro", "antigravity", "acp", "claude_sdk", "anthropic"].includes(apiType)) return [];

    const select = (key: string, label: string, options: { value: string; label: string }[], tooltip: string): AssistantFormConfig => ({
        key,
        config: {
            type: "select" as const,
            label,
            value: get(key, "") || options[0].value,
            options,
            tooltip,
            onChange: (value: string | boolean) => set(key, value === options[0].value ? "" : value, "string"),
        },
    });
    const native: AssistantFormConfig[] = {
        claude_sdk: [{
            key: "claude_permission_mode",
            config: {
                type: "select" as const,
                label: "Claude Code 默认权限模式",
                inheritedValue: "inherit",
                value: get("claude_permission_mode", "") || "inherit",
                options: [{ value: "inherit", label: "跟随提供商" }, ...claudePermissionModes],
                tooltip: "启动 Claude Code 时使用。你可以在聊天输入框中为当前会话切换权限模式。",
                onChange: (value: string | boolean) => set("claude_permission_mode", value === "inherit" ? "" : value, "string"),
            },
        }],
        pi: [select("pi_approval", "需要审批的操作", [
            { value: "all", label: "所有工具调用（默认）" },
            { value: "read_only", label: "只读工具（read / grep / find / ls）直接执行，其余审批" },
            { value: "none", label: "全部直接执行，不审批" },
        ], "Pi 自身没有权限系统，审批由 AIPP 在工具调用前加入。只读工具可以读取本机任意文件；命令、编辑、写入和 MCP 工具始终按下方方式处理。")],
        kiro: [select("kiro_autopilot", "Kiro Autopilot", [
            { value: "off", label: "关闭：逐项审批（默认）" },
            { value: "on", label: "开启：Kiro 自动执行，不再请求审批" },
        ], "Autopilot 是 Kiro 原生的自动执行模式。关闭时 Kiro 的每次权限请求都会交给 AIPP 处理。")],
        antigravity: [select("antigravity_mode", "Antigravity 权限模式", [
            { value: "default", label: "默认：逐项审批（默认）" },
            { value: "auto_edit", label: "自动接受编辑，其余审批" },
            { value: "auto", label: "自动审核（auto），高风险操作仍需确认" },
            { value: "plan", label: "规划（plan），先分析并制定计划" },
            { value: "yolo", label: "全部自动执行，跳过工具权限确认" },
        ], "全部自动执行会在 agy 启动时开启。使用独立 ACP 程序时，该程序须提供 yolo 模式。")],
        opencode: [{
            key: "opencode_approval_notice",
            config: {
                type: "static" as const,
                label: "需要审批的操作",
                value: "默认使用本机 OpenCode 权限规则。可以在聊天中选择默认工具权限，工具专属规则仍然适用。",
            },
        }],
    }[apiType === "anthropic" ? "claude_sdk" : apiType] ?? [];

    const defaultReviewMode = providerReviewMode ?? (reviewModelConfigured ? "auto_model" : "off");
    const reviewMode = get("tool_review_mode", "") || defaultReviewMode;
    return [
        ...native,
        {
            key: "tool_review_mode",
            config: {
                type: "select" as const,
                label: "审批请求的处理",
                value: reviewMode,
                inheritedValue: defaultReviewMode,
                options: [
                    { value: "off", label: "人工审批" },
                    { value: "auto_model", label: "AIPP 模型自动审核" },
                ],
                tooltip: REVIEW_TOOLTIP,
                onChange: (value: string | boolean) => set("tool_review_mode", value, "string"),
            },
        },
        ...(reviewMode === "auto_model" && !reviewModelConfigured && missingModelHint
            ? [{
                key: "tool_review_model_hint",
                config: {
                    type: "custom" as const,
                    label: "尚未配置自动审核模型，自动审核会转为需要人工确认",
                    customRender: missingModelHint,
                },
            }]
            : []),
    ];
}

/** Agent-assistant (type 4) settings persisted to assistant model_configs. Display-only notices are excluded. */
export function isAgentAssistantField(key: string): boolean {
    return key === "claude_effort" || key === "claude_permission_mode" || ["acp_", "codex_", "pi_", "kiro_", "antigravity_"].some((prefix) => key.startsWith(prefix));
}
