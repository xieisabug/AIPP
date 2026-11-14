export function agentSupportsPlanToggle(
    agentKind: string | null | undefined,
    hasSessionPlanOption: boolean,
): boolean {
    return hasSessionPlanOption
        || agentKind === "codex_app_server"
        || agentKind === "claude_sdk";
}

export interface PlanToggleVisibilityInput {
    /**
     * 当前助手是否为 Agent（assistant_type 4）。
     * false：确定不是（普通助手，或会话还在加载、还不知道是哪个助手）；
     * null：助手列表未加载，无法判断，只看会话/运行时信息。
     */
    isAgentAssistant: boolean | null;
    /** 当前助手对应的 Agent 类型（会话快照优先，其次是该助手的运行时配置） */
    agentKind: string | null | undefined;
    /** 当前会话的配置项里是否有包含 plan 的模式选项 */
    hasSessionPlanOption: boolean;
    isMobilePlatform: boolean;
}

/** 输入框下方是否显示 Plan 模式开关：只给支持 Plan 的 Agent 助手显示。 */
export function shouldShowPlanToggle({
    isAgentAssistant,
    agentKind,
    hasSessionPlanOption,
    isMobilePlatform,
}: PlanToggleVisibilityInput): boolean {
    if (isMobilePlatform || isAgentAssistant === false) return false;
    return agentSupportsPlanToggle(agentKind, hasSessionPlanOption);
}
