export type ModelRequestMode = "chat_completions" | "responses" | "anthropic_messages" | "google_gemini";

export interface ModelTagItem {
    name: string;
    code: string;
    request_mode: string;
    available_modes?: string[];
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

export const supportsRequestModeToggle = (apiType: string): boolean =>
    ["openai", "openai_api", "github_copilot", "opencode_go", "magpie"].includes(apiType);

export const requestModeLabels: Record<string, string> = {
    chat_completions: "Chat Completions",
    responses: "Responses",
    anthropic_messages: "Anthropic Messages",
    google_gemini: "Google Gemini",
    unsupported: "协议待支持",
};

export const toModelTagItem = (
    model: Pick<ModelTagItem, "name" | "code" | "request_mode" | "available_modes">,
): ModelTagItem => ({
    name: model.name,
    code: model.code,
    request_mode: model.request_mode || DEFAULT_MODEL_REQUEST_MODE,
    available_modes: model.available_modes,
});
