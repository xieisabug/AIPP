import type { AcpConversationSessionState, AgentCapabilities } from "@/data/Conversation";

/** Unknown/legacy discovery still delegates to native protocol validation. */
export function agentAllowsConfigMutation(capabilities?: AgentCapabilities | null): boolean {
    return capabilities?.operations?.set_config_option?.support !== "unsupported";
}

/** Missing or unnegotiated capability data cannot establish that restoration is unsupported. */
export function agentSessionRestoreUnsupported(state?: AcpConversationSessionState | null): boolean {
    if (!state?.session_id) return false;
    if (state.agent_capabilities) {
        const sessions = state.agent_capabilities.sessions;
        return sessions?.load?.support === "unsupported" && sessions?.resume?.support === "unsupported";
    }
    return state.load_session_supported === false && state.session_resume_supported === false;
}
