// AippAssistantTypePlugin 和 AssistantTypeApi 是全局声明的类型，不需要导入

class AcpAssistantType implements AippAssistantTypePlugin {
    onAssistantTypeInit(api: AssistantTypeApi): void {
        // 注册 ACP 助手类型 (code=100，避免与现有类型冲突)
        api.typeRegist(1, 100, "ACP 助手", this);

        // 隐藏不需要的字段
        api.hideField("model");
        api.hideField("max_tokens");
        api.hideField("temperature");
        api.hideField("top_p");
        api.hideField("stream");
        api.hideField("reasoning_effort");
        api.hideField("mcp_config");
        api.hideField("skills_config");

        // 添加 CLI 命令选择
        api.addField({
            fieldName: "acp_cli_command",
            label: "CLI 命令",
            type: "select",
            fieldConfig: {
                options: [
                    { value: "claude", label: "Claude" },
                    { value: "cursor", label: "Cursor" },
                    { value: "aider", label: "Aider" },
                ],
                tips: "选择要使用的 ACP 兼容 CLI 工具"
            }
        });

        // 添加工作目录选择（使用 custom 类型，ConfigForm 中会特殊处理）
        api.addField({
            fieldName: "acp_working_directory",
            label: "工作目录",
            type: "custom",
            fieldConfig: {
                tips: "Agent 将在此目录下运行"
            }
        });

        // 添加环境变量配置
        api.addField({
            fieldName: "acp_env_vars",
            label: "环境变量",
            type: "textarea",
            fieldConfig: {
                tips: "每行一个，格式: KEY=VALUE"
            }
        });

        // 添加附加启动参数
        api.addField({
            fieldName: "acp_additional_args",
            label: "附加启动参数",
            type: "input",
            fieldConfig: {
                tips: "传递给 CLI 的额外参数，空格分隔"
            }
        });
    }

    onAssistantTypeSelect(api: AssistantTypeApi): void {
        // 设置默认值
        api.forceFieldValue("acp_cli_command", "claude");
        api.forceFieldValue("acp_working_directory", "");
        api.forceFieldValue("acp_env_vars", "");
        api.forceFieldValue("acp_additional_args", "");
    }

    onAssistantTypeRun(_api: any): void {
        // 由后端处理 ACP 逻辑
    }
}

export default new AcpAssistantType();
