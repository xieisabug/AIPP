import consoleScript from "../../../src-tauri/artifacts/runtime/aipp-console.js?raw";

/** 预览器能渲染的类型；与后端 preview_router 发出的 `type` 一致。 */
export type ArtifactPreviewType =
    | "react"
    | "vue"
    | "mermaid"
    | "html"
    | "svg"
    | "xml"
    | "markdown"
    | "md"
    | "drawio";

const PREVIEW_TYPES: readonly ArtifactPreviewType[] = [
    "react",
    "vue",
    "mermaid",
    "html",
    "svg",
    "xml",
    "markdown",
    "md",
    "drawio",
];

export function toPreviewType(value: string | undefined | null): ArtifactPreviewType | null {
    return value && (PREVIEW_TYPES as readonly string[]).includes(value) ? (value as ArtifactPreviewType) : null;
}

/** React/Vue 需要本机预览服务器，其余类型直接在窗口内渲染。 */
export function needsDevServer(type: ArtifactPreviewType | null): type is "react" | "vue" {
    return type === "react" || type === "vue";
}

/**
 * 某个窗口的 React/Vue 预览实例 ID。须与 Rust `shared_components::preview_instance_id` 一致：
 * 主预览窗口沿用 `react`/`vue`，其他窗口加后缀，互不关闭对方的服务器。
 */
export function previewInstanceId(kind: "react" | "vue", targetWindow: string): string {
    return targetWindow === "artifact_preview" ? kind : `${kind}-${targetWindow}`;
}

/** 运行时控制台条目：`build` 来自后端构建日志，`runtime` 来自预览页面里的诊断脚本。 */
export interface ArtifactConsoleEntry {
    level: "log" | "info" | "warn" | "error" | "debug" | "success";
    message: string;
    source: "build" | "runtime";
    time: number;
}

/** 预览页面里的诊断脚本通过 postMessage 发来的消息。 */
export function readRuntimeConsoleMessage(data: unknown): ArtifactConsoleEntry | null {
    if (!data || typeof data !== "object" || !(data as { __aippArtifactConsole?: unknown }).__aippArtifactConsole) {
        return null;
    }
    const entry = (data as { entry?: { level?: unknown; message?: unknown; time?: unknown } }).entry;
    if (!entry || typeof entry.message !== "string") return null;
    const level = typeof entry.level === "string" ? entry.level : "log";
    return {
        level: (["log", "info", "warn", "error", "debug"].includes(level) ? level : "log") as ArtifactConsoleEntry["level"],
        message: entry.message,
        source: "runtime",
        time: typeof entry.time === "number" ? entry.time : Date.now(),
    };
}

/** 去掉整行注释、压成一行，注入后页面报错的行号与原文一致。规则与 Rust `inline_console_script` 一致。 */
export function compactScript(source: string): string {
    return source
        .split(/\r?\n/)
        .map((line) => line.trim())
        .filter((line) => line && !line.startsWith("//"))
        .join(" ");
}

const INLINE_CONSOLE_SCRIPT = compactScript(consoleScript);

/**
 * 把诊断脚本内联到 HTML 最前面（`<head>` 之后，否则 `<html>` 之后，否则文首），
 * 先于页面自身脚本运行。规则与 Rust `shared_components::inject_console_script` 一致。
 */
export function injectConsoleScript(html: string): string {
    const tag = `<script>${INLINE_CONSOLE_SCRIPT}</script>`;
    const insertAfter = (tagName: string): number | null => {
        const pattern = new RegExp(`<${tagName}(?=[\\s>])[^>]*>`, "i");
        const match = pattern.exec(html);
        return match ? match.index + match[0].length : null;
    };
    const index = insertAfter("head") ?? insertAfter("html");
    return index === null ? tag + html : html.slice(0, index) + tag + html.slice(index);
}

/** 代码视图高亮用的语言。 */
export function codeLanguageFor(type: ArtifactPreviewType | null): string {
    switch (type) {
        case "react":
            return "tsx";
        case "vue":
            return "vue";
        case "mermaid":
            return "mermaid";
        case "html":
            return "html";
        case "svg":
        case "xml":
        case "drawio":
            return "xml";
        case "markdown":
        case "md":
            return "markdown";
        default:
            return "text";
    }
}

/** 「保存源文件」默认扩展名。 */
export function sourceFileExtension(type: ArtifactPreviewType): string {
    switch (type) {
        case "react":
            return "tsx";
        case "md":
            return "md";
        case "markdown":
            return "md";
        case "mermaid":
            return "mmd";
        default:
            return type;
    }
}

const TYPE_LABELS: Record<ArtifactPreviewType, string> = {
    react: "React",
    vue: "Vue",
    mermaid: "Mermaid",
    html: "HTML",
    svg: "SVG",
    xml: "XML",
    markdown: "Markdown",
    md: "Markdown",
    drawio: "Draw.io",
};

export function previewTypeLabel(type: ArtifactPreviewType): string {
    return TYPE_LABELS[type];
}

/** 交给 AI 修复的报告：只带错误，相同内容只保留一次，长度受限。 */
export function buildFixReport(options: {
    title?: string;
    type: ArtifactPreviewType | null;
    entries: ArtifactConsoleEntry[];
}): string | null {
    const errors: string[] = [];
    for (const entry of options.entries) {
        if (entry.level !== "error") continue;
        const text = entry.message.trim();
        if (text && !errors.includes(text)) errors.push(text);
    }
    if (errors.length === 0) return null;
    const MAX_CHARS = 6000;
    let body = "";
    let included = 0;
    for (const error of errors) {
        const next = body ? `${body}\n\n${error}` : error;
        if (included > 0 && next.length > MAX_CHARS) break;
        body = next.length > MAX_CHARS ? `${next.slice(0, MAX_CHARS)}…` : next;
        included += 1;
    }
    if (included < errors.length) {
        body += `\n\n……另有 ${errors.length - included} 条错误未列出`;
    }
    const name = options.title ? `「${options.title}」` : "";
    const kind = options.type ? `（${previewTypeLabel(options.type)}）` : "";
    const fence = body.includes("```") ? "~~~~" : "```";
    return `Artifact${name}${kind}预览时出现以下错误，请修复：\n\n${fence}\n${body}\n${fence}`;
}
