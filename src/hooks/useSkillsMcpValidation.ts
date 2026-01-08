import { invoke } from "@tauri-apps/api/core";

/**
 * 操作 MCP 检查结果
 */
export interface OperationMcpCheckResult {
    operation_mcp_id: number | null;
    global_enabled: boolean;
    assistant_enabled: boolean;
    enabled_skills_count: number;
    agent_mcp_id?: number | null;
    agent_enabled?: boolean;
    agent_assistant_enabled?: boolean;
    agent_load_skill_enabled?: boolean;
    agent_load_skill_assistant_enabled?: boolean;
}

/**
 * 受影响助手信息
 */
export interface AffectedAssistantInfo {
    assistant_id: number;
    assistant_name: string;
    enabled_skills_count: number;
}

/**
 * 关闭操作 MCP 检查结果
 */
export interface DisableOperationMcpCheckResult {
    affected_assistants: AffectedAssistantInfo[];
}

/**
 * 操作 MCP 命令标识符
 */
export const OPERATION_MCP_COMMAND = "aipp:operation";

/**
 * Skills/MCP 校验错误类型
 */
export const OPERATION_MCP_NOT_ENABLED_ERROR = "OPERATION_MCP_NOT_ENABLED";

/**
 * Skills 与 操作 MCP 联动校验 Hook
 * 
 * 功能：
 * 1. 启用 Skills 前检查操作 MCP 是否已启用
 * 2. 关闭操作 MCP 时检查是否有 Skills 依赖
 * 3. 提供一键启用操作 MCP + Skills 的方法
 */
export function useSkillsMcpValidation() {
    const extractErrorMessage = (error: unknown): string => {
        if (!error) {
            return "";
        }
        if (typeof error === "string") {
            return error;
        }
        if (error instanceof Error) {
            return error.message || error.toString();
        }
        if (typeof error === "object") {
            const payload =
                (error as any).payload ??
                (error as any).error ??
                (error as any).cause ??
                (error as any).message;
            if (typeof payload === "string") {
                return payload;
            }
            if (payload && typeof payload === "object" && typeof (payload as any).message === "string") {
                return (payload as any).message;
            }
        }
        try {
            return JSON.stringify(error);
        } catch {
            return String(error);
        }
    };

    /**
     * 检查操作 MCP 是否已启用（用于启用 Skills 前校验）
     */
    const checkOperationMcpForSkills = async (assistantId: number): Promise<OperationMcpCheckResult> => {
        return invoke<OperationMcpCheckResult>('check_operation_mcp_for_skills', { assistantId });
    };

    /**
     * 启用操作 MCP 并启用单个 Skill
     */
    const enableOperationMcpAndSkill = async (
        assistantId: number,
        skillIdentifier: string,
        priority: number = 0
    ): Promise<void> => {
        return invoke('enable_operation_mcp_and_skill', {
            assistantId,
            skillIdentifier,
            priority
        });
    };

    /**
     * 启用操作 MCP 并批量启用 Skills
     */
    const enableOperationMcpAndSkills = async (
        assistantId: number,
        skillConfigs: [string, number][] // [skill_identifier, priority][]
    ): Promise<void> => {
        return invoke('enable_operation_mcp_and_skills', {
            assistantId,
            skillConfigs
        });
    };

    /**
     * 检查关闭全局操作 MCP 会影响哪些助手
     */
    const checkDisableOperationMcp = async (): Promise<DisableOperationMcpCheckResult> => {
        return invoke<DisableOperationMcpCheckResult>('check_disable_operation_mcp');
    };

    /**
     * 关闭全局操作 MCP 并同时关闭所有助手的 Skills
     */
    const disableOperationMcpWithSkills = async (): Promise<void> => {
        return invoke('disable_operation_mcp_with_skills');
    };

    /**
     * 检查关闭助手级操作 MCP 会影响多少 Skills
     */
    const checkDisableAssistantOperationMcp = async (assistantId: number): Promise<number> => {
        return invoke<number>('check_disable_assistant_operation_mcp', { assistantId });
    };

    /**
     * 关闭助手级操作 MCP 并同时关闭该助手的 Skills
     */
    const disableAssistantOperationMcpWithSkills = async (assistantId: number): Promise<void> => {
        return invoke('disable_assistant_operation_mcp_with_skills', { assistantId });
    };

    /**
     * 判断错误是否为操作 MCP 未启用
     */
    const isOperationMcpNotEnabledError = (error: unknown): boolean => {
        const message = extractErrorMessage(error);
        return message === OPERATION_MCP_NOT_ENABLED_ERROR || message.includes(OPERATION_MCP_NOT_ENABLED_ERROR);
    };

    /**
     * 判断 MCP 服务器是否为操作工具集
     */
    const isOperationMcp = (mcpCommand: string | undefined | null): boolean => {
        return mcpCommand === OPERATION_MCP_COMMAND;
    };

    return {
        // 检查 API
        checkOperationMcpForSkills,
        checkDisableOperationMcp,
        checkDisableAssistantOperationMcp,
        // 启用 API
        enableOperationMcpAndSkill,
        enableOperationMcpAndSkills,
        // 关闭 API
        disableOperationMcpWithSkills,
        disableAssistantOperationMcpWithSkills,
        // 辅助函数
        isOperationMcpNotEnabledError,
        isOperationMcp,
    };
}

export default useSkillsMcpValidation;
