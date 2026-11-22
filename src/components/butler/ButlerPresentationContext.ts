import { createContext } from "react";

// Keep execution widgets out of compact chat even during streaming or renderer fallback.
export const ButlerResultsOnlyContext = createContext(false);

const interactionTools = new Set(["preview_file", "preview_code", "ask_user_question", "present_choices"]);

export function isButlerInteractionTool(name?: string) {
    return interactionTools.has(name ?? "");
}

/** 结果类工具：卡片就是交付给用户的内容（引用的回复、生成的文件、发布的 Artifact），简洁聊天也保留。 */
const resultTools = new Set(["preview_message", "office_edit", "show_artifact"]);

export function isResultCardTool(name?: string) {
    return resultTools.has(name ?? "");
}

// The conversation already renders these interactions through live request hooks.
export const ButlerLiveCardsContext = createContext<{ file?: number | null; question?: number | null }>({});
