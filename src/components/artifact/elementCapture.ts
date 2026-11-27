import { invoke } from "@tauri-apps/api/core";

interface CaptureSession {
    artifactKey?: string | null;
    currentVersion?: number;
    versions: { version: number }[];
}

/**
 * 「选择元素修改」的元素截图：后台按选择器截取会话 Artifact 工作区里的当前文件。
 * 只在看的是最新版本时提供（旧版本的文件已不在工作区，截出来会对不上）。
 */
export function elementCapture(
    conversationId: number | undefined,
    session: CaptureSession,
): ((selector: string, size: { width: number; height: number }) => Promise<string>) | undefined {
    const artifactKey = session.artifactKey;
    if (conversationId === undefined || !artifactKey) return undefined;
    const latest = session.versions[0]?.version;
    if (latest !== undefined && session.currentVersion !== undefined && session.currentVersion !== latest) return undefined;
    return (selector, size) =>
        invoke<string>("capture_artifact_element", {
            conversationId,
            artifactKey,
            selector,
            width: size.width,
            height: size.height,
        });
}
