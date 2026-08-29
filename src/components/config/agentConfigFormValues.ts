export interface AgentConfigFormValues {
    codex_notification_on_success: boolean;
    claude_code_notification_on_success: boolean;
    acp_notification_on_success: boolean;
}

export const buildAgentConfigFormValues = (
    config?: Map<string, string>
): AgentConfigFormValues => ({
    codex_notification_on_success:
        config?.get("codex_notification_on_success") === "true",
    claude_code_notification_on_success:
        config?.get("claude_code_notification_on_success") === "true",
    acp_notification_on_success:
        config?.get("acp_notification_on_success") === "true",
});

export const serializeAgentConfigFormValues = (
    values: AgentConfigFormValues
): Record<string, string> => ({
    codex_notification_on_success: String(values.codex_notification_on_success),
    claude_code_notification_on_success: String(values.claude_code_notification_on_success),
    acp_notification_on_success: String(values.acp_notification_on_success),
});
