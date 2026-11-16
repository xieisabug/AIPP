import { createContext } from "react";

// Keep execution widgets out of compact chat even during streaming or renderer fallback.
export const ButlerResultsOnlyContext = createContext(false);

const interactionTools = new Set(["preview_file", "preview_code", "ask_user_question"]);

export function isButlerInteractionTool(name?: string) {
    return interactionTools.has(name ?? "");
}

// The conversation already renders these interactions through live request hooks.
export const ButlerLiveCardsContext = createContext<{ file?: number | null; question?: number | null }>({});
