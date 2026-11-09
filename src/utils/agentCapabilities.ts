import type { AgentCapabilities } from "@/data/Conversation";

/** Unknown/legacy discovery still delegates to native protocol validation. */
export function agentAllowsConfigMutation(capabilities?: AgentCapabilities | null): boolean {
    return capabilities?.operations?.set_config_option?.support !== "unsupported";
}
