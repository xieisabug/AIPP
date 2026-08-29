export interface AgentModelOption {
    code: string;
    name: string;
    provider_id: number;
    efforts: string[];
    default_effort?: string | null;
    is_default?: boolean;
}

export function resolveAgentDefaultSelection(
    models: AgentModelOption[],
    configuredModelCode?: string | null,
    configuredEffort?: string | null,
) {
    const configuredModel = configuredModelCode
        ? models.find((model) => model.code.startsWith(`${configuredModelCode}%%`))
        : undefined;
    const model = configuredModel ?? models.find((option) => option.is_default) ?? models[0];
    const efforts = model?.efforts ?? [];
    const effort = model?.default_effort && efforts.includes(model.default_effort)
        ? model.default_effort
        : configuredModel && configuredEffort && efforts.includes(configuredEffort)
            ? configuredEffort
            : (efforts[0] ?? "");
    return { model, efforts, effort };
}
