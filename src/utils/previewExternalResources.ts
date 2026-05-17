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

export function getActionablePreviewResources(
    externalResources?: PreviewExternalResourcesPayload | null
) {
    return externalResources?.resources.filter((resource) =>
        resource.status === "pending"
        || resource.status === "blocked"
        || resource.status === "failed"
    ) ?? [];
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
