import { invoke } from "@tauri-apps/api/core";
import {
    SubTaskDefinition,
    SubTaskExecutionSummary,
    SubTaskExecutionDetail,
    CreateSubTaskRequest,
    MCPToolCallUI,
    RegisterSubTaskDefinitionRequest,
    UpdateSubTaskDefinitionRequest,
    ListSubTaskDefinitionsParams,
    ListSubTaskExecutionsParams,
    SubTaskService,
} from "../data/SubTask";
import { Clock, Zap, CheckCircle, XCircle, Square, HelpCircle } from "lucide-react";
import React from "react";
import { getSubTaskIconComponent } from "../data/SubTask";

class SubTaskServiceImpl implements SubTaskService {
    // 任务定义管理
    async registerDefinition(request: RegisterSubTaskDefinitionRequest): Promise<number> {
        return invoke<number>("register_sub_task_definition", {
            name: request.name,
            code: request.code,
            description: request.description,
            systemPrompt: request.system_prompt,
            pluginSource: request.plugin_source,
            sourceId: request.source_id,
        });
    }

    async listDefinitions(params?: ListSubTaskDefinitionsParams): Promise<SubTaskDefinition[]> {
        const definitions = await invoke<SubTaskDefinition[]>("list_sub_task_definitions", {
            pluginSource: params?.plugin_source,
            sourceId: params?.source_id,
            isEnabled: params?.is_enabled,
        });

        // 转换日期字段
        return definitions.map((def) => ({
            ...def,
            created_time: new Date(def.created_time),
            updated_time: new Date(def.updated_time),
            // 合并运行期注册的组件图标
            iconComponent: getSubTaskIconComponent(def.code),
        }));
    }

    async getDefinition(code: string, source_id: number): Promise<SubTaskDefinition | null> {
        const definition = await invoke<SubTaskDefinition | null>("get_sub_task_definition", {
            code,
            sourceId: source_id,
        });

        if (!definition) return null;

        return {
            ...definition,
            created_time: new Date(definition.created_time),
            updated_time: new Date(definition.updated_time),
            // 合并运行期注册的组件图标
            iconComponent: getSubTaskIconComponent(definition.code),
        };
    }

    async updateDefinition(request: UpdateSubTaskDefinitionRequest): Promise<void> {
        await invoke<void>("update_sub_task_definition", {
            id: request.id,
            name: request.name,
            description: request.description,
            systemPrompt: request.system_prompt,
            isEnabled: request.is_enabled,
            sourceId: request.source_id,
        });
    }

    async deleteDefinition(id: number, source_id: number): Promise<void> {
        await invoke<void>("delete_sub_task_definition", {
            id,
            sourceId: source_id,
        });
    }

    // 任务执行管理
    async createExecution(request: CreateSubTaskRequest): Promise<number> {
        return invoke<number>("create_sub_task_execution", { request });
    }

    async listExecutions(params: ListSubTaskExecutionsParams): Promise<SubTaskExecutionSummary[]> {
        const executions = await invoke<SubTaskExecutionSummary[]>("list_sub_task_executions", {
            parentConversationId: params.parent_conversation_id,
            parentMessageId: params.parent_message_id,
            status: params.status,
            sourceId: params.source_id,
            page: params.page,
            pageSize: params.page_size,
        });

        // 转换日期字段
        return executions.map((exec) => ({
            ...exec,
            created_time: new Date(exec.created_time),
        }));
    }

    async getExecutionDetail(execution_id: number, source_id: number): Promise<SubTaskExecutionDetail | null> {
        const execution = await invoke<SubTaskExecutionDetail | null>("get_sub_task_execution_detail", {
            executionId: execution_id,
            sourceId: source_id,
        });

        if (!execution) return null;

        return {
            ...execution,
            created_time: new Date(execution.created_time),
            started_time: execution.started_time ? new Date(execution.started_time) : undefined,
            finished_time: execution.finished_time ? new Date(execution.finished_time) : undefined,
        };
    }

    // UI展示用的获取详情方法（不需要鉴权）
    async getExecutionDetailForUI(execution_id: number): Promise<SubTaskExecutionDetail | null> {
        const execution = await invoke<any | null>("get_sub_task_execution_detail_for_ui", {
            executionId: execution_id,
        });

        if (!execution) return null;

        const mapped: any = {
            ...execution,
            created_time: new Date(execution.created_time),
            started_time: execution.started_time ? new Date(execution.started_time) : undefined,
            finished_time: execution.finished_time ? new Date(execution.finished_time) : undefined,
        };
        return mapped as SubTaskExecutionDetail;
    }

    // 获取某次子任务的 MCP 工具调用列表（UI专用）
    async getMcpCallsForExecution(execution_id: number): Promise<MCPToolCallUI[]> {
        const calls = await invoke<any[]>("get_sub_task_mcp_calls_for_ui", {
            executionId: execution_id,
        });
        return calls.map((c) => ({
            ...c,
            created_time: new Date(c.created_time),
            started_time: c.started_time ? new Date(c.started_time) : undefined,
            finished_time: c.finished_time ? new Date(c.finished_time) : undefined,
        }));
    }

    // UI专用的取消任务方法（不需要鉴权）
    async cancelExecutionForUI(execution_id: number): Promise<void> {
        await invoke<void>("cancel_sub_task_execution_for_ui", {
            executionId: execution_id,
        });
    }

    async cancelExecution(execution_id: number, source_id: number): Promise<void> {
        await invoke<void>("cancel_sub_task_execution", {
            executionId: execution_id,
            sourceId: source_id,
        });
    }
}

// 单例服务实例
export const subTaskService = new SubTaskServiceImpl();

// 工具函数
export const getStatusColor = (status: string): string => {
    switch (status) {
        case "pending":
            return "text-yellow-600 bg-yellow-100";
        case "running":
            return "text-blue-600 bg-blue-100";
        case "success":
            return "text-green-600 bg-green-100";
        case "failed":
            return "text-red-600 bg-red-100";
        case "cancelled":
            return "text-gray-600 bg-gray-100";
        default:
            return "text-gray-600 bg-gray-100";
    }
};

export const getStatusIcon = (status: string): React.ReactElement => {
    const iconProps = { className: "w-3 h-3" };

    switch (status) {
        case "pending":
            return React.createElement(Clock, iconProps);
        case "running":
            return React.createElement(Zap, iconProps);
        case "success":
            return React.createElement(CheckCircle, iconProps);
        case "failed":
            return React.createElement(XCircle, iconProps);
        case "cancelled":
            return React.createElement(Square, iconProps);
        default:
            return React.createElement(HelpCircle, iconProps);
    }
};

export const getStatusText = (status: string): string => {
    switch (status) {
        case "pending":
            return "等待中";
        case "running":
            return "执行中";
        case "success":
            return "成功";
        case "failed":
            return "失败";
        case "cancelled":
            return "已取消";
        default:
            return "未知";
    }
};

export const formatTokenCount = (count: number): string => {
    if (count === 0) return "0";
    if (count < 1000) return count.toString();
    if (count < 1000000) return `${(count / 1000).toFixed(1)}K`;
    return `${(count / 1000000).toFixed(1)}M`;
};

export const formatDuration = (startTime?: Date, endTime?: Date): string => {
    if (!startTime) return "-";

    const end = endTime || new Date();
    const duration = end.getTime() - startTime.getTime();

    if (duration < 1000) return "< 1秒";
    if (duration < 60000) return `${Math.floor(duration / 1000)}秒`;
    if (duration < 3600000) return `${Math.floor(duration / 60000)}分钟`;
    return `${Math.floor(duration / 3600000)}小时`;
};
