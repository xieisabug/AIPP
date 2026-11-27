import { useSyncExternalStore } from "react";

/**
 * 聊天窗口里的 Artifact 分栏：右侧固定一个预览面板，边对话边看效果。
 *
 * 只有聊天窗口调用 `enableArtifactPane(true)`（每个窗口是独立的 JS 环境），其他窗口里的同一组件仍打开独立预览窗口。
 * 预览入口（消息里的「运行」、Artifact 工具卡片、比选卡片）调用 `artifactPreviewTarget()`：
 * 分栏可用时先打开面板，再把 `sourceWindow: "chat_ui"` 交给 `run_artifacts`，内容就显示在面板里。
 */
export const ARTIFACT_PANE_WINDOW = "chat_ui";
/** 窗口太窄时分栏挤不下，改用独立预览窗口。 */
export const MIN_WINDOW_WIDTH_FOR_PANE = 900;

let enabled = false;
let open = false;
const listeners = new Set<() => void>();

function setOpen(next: boolean) {
    if (open === next) return;
    open = next;
    listeners.forEach((notify) => notify());
}

export function enableArtifactPane(value: boolean) {
    enabled = value;
    if (!value) setOpen(false);
}

export function closeArtifactPane() {
    setOpen(false);
}

/** 交给 `run_artifacts` 的目标窗口参数；分栏可用时顺带打开面板。 */
export function artifactPreviewTarget(windowWidth: number = typeof window === "undefined" ? 0 : window.innerWidth): {
    sourceWindow?: string;
} {
    if (!enabled || windowWidth < MIN_WINDOW_WIDTH_FOR_PANE) return {};
    setOpen(true);
    return { sourceWindow: ARTIFACT_PANE_WINDOW };
}

export function useArtifactPaneOpen(): boolean {
    return useSyncExternalStore(
        (notify) => {
            listeners.add(notify);
            return () => listeners.delete(notify);
        },
        () => open,
    );
}
