export function agentSupportsPlanToggle(
    agentKind: string | null | undefined,
    hasSessionPlanOption: boolean,
): boolean {
    return hasSessionPlanOption
        || agentKind === "codex_app_server"
        || agentKind === "claude_sdk";
}
