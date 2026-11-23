export type PreviewExternalResourceType =
    | "image"
    | "css"
    | "script"
    | "font"
    | "pdf"
    | "html"
    | "text"
    | "markdown"
    | "media"
    | "unknown";

export type PreviewExternalResourceStatus = "allowed" | "pending" | "blocked" | "failed";
export type PreviewExternalResourceRisk = "low" | "medium" | "high";

export interface PreviewExternalResource {
    id: string;
    originalUrl: string;
    normalizedUrl: string;
    type: PreviewExternalResourceType;
    source: "preview_code" | "preview_file" | string;
    occurrence: string;
    status: PreviewExternalResourceStatus;
    allowedBy?: "whitelist" | "user";
    risk: PreviewExternalResourceRisk;
    reason?: string;
    /** 引用它的外部样式表地址；直接写在预览内容里的资源为空。 */
    via?: string;
}

export interface PreviewExternalResourcesPayload {
    requestId: string;
    resources: PreviewExternalResource[];
}

export interface PreviewResourceAuthorizationResult<TCode = unknown, TFile = unknown> {
    previewCode?: TCode;
    previewFile?: TFile;
}

export function hasActionablePreviewResources(
    externalResources?: PreviewExternalResourcesPayload | null
) {
    return getActionablePreviewResources(externalResources).length > 0;
}

/** 可以授权（或重试）的资源。「已拦截」是无法解析的地址，授权也加载不了，不算在内。 */
export function getActionablePreviewResources(
    externalResources?: PreviewExternalResourcesPayload | null
) {
    return externalResources?.resources.filter((resource) =>
        resource.status === "pending"
        || resource.status === "failed"
    ) ?? [];
}

export function getBlockedPreviewResources(
    externalResources?: PreviewExternalResourcesPayload | null
) {
    return externalResources?.resources.filter((resource) => resource.status === "blocked") ?? [];
}

const typeLabels: Record<PreviewExternalResourceType, string> = {
    image: "图片",
    css: "样式表",
    script: "脚本",
    font: "字体",
    pdf: "PDF",
    html: "网页",
    text: "文本",
    markdown: "Markdown",
    media: "音视频",
    unknown: "其他",
};

export function previewResourceTypeLabel(type: PreviewExternalResourceType) {
    return typeLabels[type] ?? type;
}

/** 远程地址显示主机名，其余是本机文件。 */
export function previewResourceHost(url: string) {
    try {
        const parsed = new URL(url);
        return /^https?:$/.test(parsed.protocol) ? parsed.hostname : "本机文件";
    } catch {
        return "本机文件";
    }
}

function shortUrl(url: string) {
    try {
        const parsed = new URL(url);
        return parsed.host + parsed.pathname;
    } catch {
        return url;
    }
}

const tagLabels: Record<string, string> = {
    img: "页面里的图片",
    image: "页面里的图片",
    link: "页面引用的样式表",
    script: "页面引用的脚本",
    video: "页面里的音视频",
    audio: "页面里的音视频",
    source: "页面里的音视频",
    iframe: "页面嵌入的网页",
    object: "页面嵌入的网页",
    embed: "页面嵌入的网页",
};

const occurrenceLabels: Record<string, string> = {
    "style block": "页面 <style> 里的样式",
    "inline style": "元素的内联样式",
    "css url": "样式里的 url()",
    "css import": "样式里的 @import",
    "css secondary": "外部样式表引用",
    "markdown image": "Markdown 图片",
    "file url": "预览文件地址",
    "selected resource": "所选资源",
};

function occurrenceLabel(occurrence: string) {
    const tag = /^<?([a-z]+)>?\s+[a-z]+$/i.exec(occurrence)?.[1]?.toLowerCase();
    return (tag && tagLabels[tag]) || occurrenceLabels[occurrence] || occurrence;
}

/** 资源出现在哪里，例如「由样式表 cdn.jsdelivr.net/x/index.css 引用」。 */
export function describePreviewResourceOrigin(resource: PreviewExternalResource) {
    if (resource.via) return `由样式表 ${shortUrl(resource.via)} 引用`;
    const labels = resource.occurrence
        .split(",")
        .map((item) => occurrenceLabel(item.trim()))
        .filter(Boolean);
    return Array.from(new Set(labels)).join("、");
}

export function getDefaultSelectedPreviewResourceIds(
    externalResources?: PreviewExternalResourcesPayload | null
) {
    return new Set(
        getActionablePreviewResources(externalResources)
            .filter((resource) =>
                resource.risk !== "high"
            )
            .map((resource) => resource.id) ?? []
    );
}
