/** Built-in fields share one classification across stored configs and the renderer. */
export const ASSISTANT_ADVANCED_FIELD_KEYS = [
    "max_tokens", "temperature", "top_p", "stream", "reasoning_effort",
    "dynamic_mcp_loading_enabled", "use_native_toolcall", "tool_review_mode",
    "tool_review_model_hint", "workspace_config", "skills_config", "mcp_config",
    "acp_env_vars", "acp_additional_args", "codex_approval_policy", "codex_sandbox",
    "codex_approvals_reviewer",
];

const BUILTIN_FIELD_KEYS = new Set([
    "assistantType", "model", "prompt", "acp_provider", "agent_model", "acp_working_directory",
    ...ASSISTANT_ADVANCED_FIELD_KEYS,
]);

// Legacy plugins stored their own fields in model_configs without registration metadata.
// Preserve those fields, but keep them with plugin settings rather than basic configuration.
export const isAssistantExtensionConfig = (key: string) => !BUILTIN_FIELD_KEYS.has(key);
