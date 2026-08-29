import React, { useCallback } from "react";
import { UseFormReturn } from "react-hook-form";
import { toast } from "sonner";
import ConfigForm from "@/components/ConfigForm";
import { AgentConfigFormValues } from "../../agentConfigFormValues";

interface AgentConfigFormProps {
    form: UseFormReturn<AgentConfigFormValues>;
    onSave: () => Promise<void>;
}

export const AgentConfigForm: React.FC<AgentConfigFormProps> = ({ form, onSave }) => {
    const handleSave = useCallback(async () => {
        try {
            await onSave();
            toast.success("Agent 配置保存成功");
        } catch (error) {
            toast.error("保存 Agent 配置失败: " + error);
        }
    }, [onSave]);

    return (
        <ConfigForm
            title="Agent"
            description="配置各 Agent 成功完成任务后的系统通知"
            config={[
                {
                    key: "codex_notification_on_success",
                    config: {
                        type: "switch",
                        label: "Codex 完成后通知",
                        tooltip: "Codex 成功完成一轮任务后，在聊天窗口未聚焦时发送系统通知",
                    },
                },
                {
                    key: "claude_code_notification_on_success",
                    config: {
                        type: "switch",
                        label: "Claude Code 完成后通知",
                        tooltip: "Claude Code 成功完成一轮任务后，在聊天窗口未聚焦时发送系统通知",
                    },
                },
                {
                    key: "acp_notification_on_success",
                    config: {
                        type: "switch",
                        label: "ACP 完成后通知",
                        tooltip: "ACP Agent 成功完成一轮任务后，在聊天窗口未聚焦时发送系统通知",
                    },
                },
            ]}
            useFormReturn={form}
            onSave={handleSave}
        />
    );
};
