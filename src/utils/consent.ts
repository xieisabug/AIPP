import { confirmDialog, type ConfirmOptions } from "@/components/common/ConfirmHost";

/** 对应 Rust `utils::consent::Consent<T>`。 */
export type Consent<T> = { status: "done"; value: T } | { status: "confirm"; prompt: string };

/**
 * 执行需要用户确认的后端命令：后端返回确认文案时用应用内确认框询问，
 * 同意后把文案原样传回重试（文案变化会再问一次）。用户取消返回 `null`。
 */
export async function withConsent<T>(
    run: (confirmedPrompt: string | null) => Promise<Consent<T>>,
    dialog: Omit<ConfirmOptions, "description">,
): Promise<{ value: T } | null> {
    let confirmed: string | null = null;
    for (;;) {
        const result = await run(confirmed);
        if (result.status === "done") return { value: result.value };
        if (!await confirmDialog({ ...dialog, description: result.prompt })) return null;
        confirmed = result.prompt;
    }
}

/** Bun 安装依赖并构建工程前的确认框文案（比选工程、收藏、多文件 Artifact 共用）。 */
export const PROJECT_BUILD_CONFIRM = { title: "确认安装依赖并构建工程", confirmText: "安装并构建" } as const;
