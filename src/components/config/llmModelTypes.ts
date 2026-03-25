export type ModelRequestMode = "chat_completions" | "responses";

export interface ModelTagItem {
    name: string;
    code: string;
    request_mode: string;
}

export interface LLMModel extends ModelTagItem {
    id: number;
    llm_provider_id: number;
    description: string;
    vision_support: boolean;
    audio_support: boolean;
    video_support: boolean;
}

export interface ModelForSelection extends ModelTagItem {
    description: string;
    vision_support: boolean;
    audio_support: boolean;
    video_support: boolean;
    is_selected: boolean;
}

export interface ModelSelectionResponse {
    available_models: ModelForSelection[];
    missing_models: string[];
}

export const DEFAULT_MODEL_REQUEST_MODE: ModelRequestMode = "chat_completions";

export const normalizeRequestMode = (requestMode?: string): ModelRequestMode =>
    requestMode === "responses" ? "responses" : "chat_completions";

export const toggleRequestMode = (requestMode?: string): ModelRequestMode =>
    normalizeRequestMode(requestMode) === "responses" ? "chat_completions" : "responses";

export const supportsRequestModeToggle = (apiType: string): boolean =>
    ["openai", "openai_api", "github_copilot"].includes(apiType);

export const getRequestModeLabel = (requestMode?: string): "c" | "r" =>
    normalizeRequestMode(requestMode) === "responses" ? "r" : "c";

export const getRequestModeTooltip = (requestMode?: string): string =>
    normalizeRequestMode(requestMode) === "responses"
        ? "当前使用 Responses 接口，点击切换到 Chat Completions"
        : "当前使用 Chat Completions 接口，点击切换到 Responses";

export const toModelTagItem = (
    model: Pick<ModelTagItem, "name" | "code" | "request_mode">,
): ModelTagItem => ({
    name: model.name,
    code: model.code,
    request_mode: normalizeRequestMode(model.request_mode),
});
