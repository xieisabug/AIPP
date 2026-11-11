import React from "react";
import type { AssistantFormConfig } from "@/types/forms";

type ConfigGetter = (name: string, defaultValue: string) => string;
type ConfigSetter = (key: string, value: string | boolean, valueType: string) => void;

const REVIEW_TOOLTIP =
    "智能体请求执行操作时：人工审批会弹出确认；AIPP 模型自动审核先由「设置 → 辅助AI」中的审核模型判断，安全的直接按「本次允许」执行（不会替你选「始终允许」），有风险或审核失败才弹出确认并说明原因。";

/**
 * Approval settings for Pi / OpenCode / Kiro / Antigravity / Cursor agent assistants.
 * Each agent's native control plus AIPP's model review for agents without their own reviewer.
 * Empty stored values mean the safe default (every action asks).
 */
export function buildNativeAgentApprovalFields(
    apiType: string,
    get: ConfigGetter,
    set: ConfigSetter,
    reviewModelConfigured: boolean,
    missingModelHint?: () => React.ReactNode,
): AssistantFormConfig[] {
    if (apiType === "cursor") {
        return [{
            key: "cursor_approval_notice",
            config: {
                type: "static" as const,
                label: "审批",
                value: "Cursor 使用 SDK 自带的自动审核（Auto-review）决定能否执行，SDK 不支持逐项人工审批，因此不会弹出确认。",
            },
        }];
    }
    if (!["pi", "opencode", "kiro", "antigravity"].includes(apiType)) return [];

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
            { value: "yolo", label: "全部自动执行（yolo）" },
        ], "使用 Antigravity 原生权限模式；所选模式未被当前 agy 版本提供时会直接报错并列出可用模式。")],
        opencode: [{
            key: "opencode_approval_notice",
            config: {
                type: "static" as const,
                label: "需要审批的操作",
                value: "由本机 OpenCode 配置中的 permissions 规则决定（默认几乎全部放行，仅对 .env 等敏感读取请求审批）。",
            },
        }],
    }[apiType] ?? [];

    const reviewMode = get("tool_review_mode", "") === "auto_model" ? "auto_model" : "off";
    return [
        ...native,
        {
            key: "tool_review_mode",
            config: {
                type: "select" as const,
                label: "审批请求的处理",
                value: reviewMode,
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
    return ["acp_", "codex_", "pi_", "kiro_", "antigravity_"].some((prefix) => key.startsWith(prefix));
}
