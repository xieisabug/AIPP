import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, ShieldCheck } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { getErrorMessage } from "@/utils/error";
import {
    getActionablePreviewResources,
    getDefaultSelectedPreviewResourceIds,
    PreviewExternalResourcesPayload,
    PreviewResourceAuthorizationResult,
} from "@/utils/previewExternalResources";

interface PreviewExternalResourcesDialogProps<TCode, TFile> {
    externalResources?: PreviewExternalResourcesPayload | null;
    open: boolean;
    onOpenChange: (open: boolean) => void;
    onAuthorized: (result: PreviewResourceAuthorizationResult<TCode, TFile>) => void;
    isLoading?: boolean;
    scanError?: string | null;
    onRetryScan?: () => void;
    canAuthorize?: boolean;
    onAuthorizeSelected?: (
        selectedResourceIds: string[],
        options: { addToWhitelist: boolean; useProxy: boolean }
    ) => Promise<PreviewResourceAuthorizationResult<TCode, TFile>>;
}

const riskLabel = {
    low: "低风险",
    medium: "中风险",
    high: "高风险",
};

const statusLabel = {
    allowed: "已允许",
    pending: "待授权",
    blocked: "已拦截",
    failed: "加载失败",
};

function getHostname(rawUrl: string) {
    try {
        return new URL(rawUrl).hostname;
    } catch {
        return "本地或未知来源";
    }
}

function getResultExternalResources<TCode, TFile>(
    result: PreviewResourceAuthorizationResult<TCode, TFile>
) {
    const previewCode = result.previewCode as
        | { externalResources?: PreviewExternalResourcesPayload | null }
        | undefined;
    const previewFile = result.previewFile as
        | { externalResources?: PreviewExternalResourcesPayload | null }
        | undefined;
    return previewCode?.externalResources ?? previewFile?.externalResources ?? null;
}

export default function PreviewExternalResourcesDialog<TCode, TFile>({
    externalResources,
    open,
    onOpenChange,
    onAuthorized,
    isLoading = false,
    scanError = null,
    onRetryScan,
    canAuthorize = true,
    onAuthorizeSelected,
}: PreviewExternalResourcesDialogProps<TCode, TFile>) {
    const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
    const [isSubmitting, setIsSubmitting] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [localResources, setLocalResources] = useState<PreviewExternalResourcesPayload | null>(null);

    useEffect(() => {
        if (!open) {
            setError(null);
            setLocalResources(null);
            return;
        }
        setLocalResources(null);
        setSelectedIds(getDefaultSelectedPreviewResourceIds(externalResources));
    }, [externalResources, open]);

    const effectiveResources = localResources ?? externalResources ?? null;
    const resources = getActionablePreviewResources(effectiveResources);
    const visibleError = error ?? scanError;
    const selectableIds = useMemo(
        () =>
            new Set(
                resources
                    .map((resource) => resource.id)
            ),
        [resources]
    );
    const selectedResourceIds = Array.from(selectedIds).filter((id) => selectableIds.has(id));
    const selectedCount = selectedResourceIds.length;

    const submit = async (addToWhitelist: boolean, useProxy = false) => {
        if (!effectiveResources || selectedCount === 0) {
            return;
        }
        setIsSubmitting(true);
        setError(null);
        try {
            const result = onAuthorizeSelected
                ? await onAuthorizeSelected(selectedResourceIds, { addToWhitelist, useProxy })
                : await invoke<PreviewResourceAuthorizationResult<TCode, TFile>>(
                    "authorize_preview_external_resources",
                    {
                        requestId: effectiveResources.requestId,
                        request_id: effectiveResources.requestId,
                        resourceIds: selectedResourceIds,
                        resource_ids: selectedResourceIds,
                        addToWhitelist,
                        add_to_whitelist: addToWhitelist,
                          useProxy,
                          use_proxy: useProxy,
                      }
                  );
            onAuthorized(result);
            const nextExternalResources = getResultExternalResources(result);
            const nextResources = getActionablePreviewResources(nextExternalResources);
            if (nextResources.length > 0) {
                setLocalResources(nextExternalResources);
                setSelectedIds(getDefaultSelectedPreviewResourceIds(nextExternalResources));
                setError(
                    nextResources.some((resource) => resource.status === "failed")
                        ? "部分资源加载失败，请检查代理或网络后重试。"
                        : null
                );
                return;
            }
            setLocalResources(null);
            onOpenChange(false);
        } catch (submitError) {
            setError(getErrorMessage(submitError));
        } finally {
            setIsSubmitting(false);
        }
    };

    return (
        <Dialog open={open} onOpenChange={onOpenChange}>
            <DialogContent className="sm:max-w-3xl">
                <DialogHeader>
                    <DialogTitle className="flex items-center gap-2">
                        <ShieldCheck className="h-5 w-5" />
                        外部资源授权
                    </DialogTitle>
                    <DialogDescription>
                        预览中的外部资源已被拦截。只允许所选资源通过本地 preview relay 加载。
                    </DialogDescription>
                </DialogHeader>

                {visibleError && (
                    <div className="rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm text-destructive">
                        {visibleError}
                    </div>
                )}

                {isLoading && resources.length === 0 ? (
                    <div className="rounded-md border border-border bg-muted/30 px-3 py-6 text-center text-sm text-muted-foreground">
                        正在检测外部资源...
                    </div>
                ) : resources.length === 0 ? (
                    <div className="rounded-md border border-border bg-muted/30 px-3 py-6 text-center text-sm text-muted-foreground">
                        当前预览没有待处理的外部资源。
                    </div>
                ) : (
                    <ScrollArea className="max-h-[56vh] pr-3">
                        <div className="space-y-3">
                            {resources.map((resource) => {
                                const selectable = selectableIds.has(resource.id);
                                const checked = selectedIds.has(resource.id);
                                return (
                                    <label
                                        key={resource.id}
                                        className="flex gap-3 rounded-md border border-border p-3"
                                    >
                                        <Checkbox
                                            className="mt-1"
                                            checked={checked}
                                            disabled={!selectable || isSubmitting}
                                            onCheckedChange={(value) => {
                                                setSelectedIds((current) => {
                                                    const next = new Set(current);
                                                    if (value === true) {
                                                        next.add(resource.id);
                                                    } else {
                                                        next.delete(resource.id);
                                                    }
                                                    return next;
                                                });
                                            }}
                                        />
                                        <div className="min-w-0 flex-1 space-y-2">
                                            <div className="flex flex-wrap items-center gap-2">
                                                <Badge variant="outline">{resource.type}</Badge>
                                                <Badge
                                                    variant={resource.risk === "high" ? "destructive" : "secondary"}
                                                    className="gap-1"
                                                >
                                                    {resource.risk === "high" && (
                                                        <AlertTriangle className="h-3 w-3" />
                                                    )}
                                                    {riskLabel[resource.risk]}
                                                </Badge>
                                                <Badge variant="outline">
                                                    {statusLabel[resource.status]}
                                                </Badge>
                                                <span className="text-xs text-muted-foreground">
                                                    {getHostname(resource.normalizedUrl)}
                                                </span>
                                            </div>
                                            <div className="text-xs text-muted-foreground">
                                                {resource.source} · {resource.occurrence}
                                            </div>
                                            <div className="whitespace-pre-wrap break-all rounded border border-border bg-muted/40 px-2 py-1 text-xs font-mono text-foreground">
                                                {resource.originalUrl}
                                            </div>
                                            {resource.reason && (
                                                <div className="text-xs text-destructive">
                                                    {resource.reason}
                                                </div>
                                            )}
                                        </div>
                                    </label>
                                );
                            })}
                        </div>
                    </ScrollArea>
                )}

                <DialogFooter>
                    {onRetryScan && resources.length === 0 && (
                        <Button
                            type="button"
                            variant="outline"
                            onClick={onRetryScan}
                            disabled={isSubmitting || isLoading}
                        >
                            重新检测
                        </Button>
                    )}
                    <Button
                        type="button"
                        variant="outline"
                        onClick={() => onOpenChange(false)}
                        disabled={isSubmitting}
                    >
                        取消
                    </Button>
                    <Button
                        type="button"
                        variant="outline"
                        disabled={selectedCount === 0 || isSubmitting || !canAuthorize}
                        onClick={() => void submit(true)}
                    >
                        加入白名单并加载
                    </Button>
                    <Button
                        type="button"
                        variant="outline"
                        disabled={selectedCount === 0 || isSubmitting || !canAuthorize}
                        onClick={() => void submit(false, true)}
                    >
                        使用代理加载所选资源
                    </Button>
                    <Button
                        type="button"
                        disabled={selectedCount === 0 || isSubmitting || !canAuthorize}
                        onClick={() => void submit(false)}
                    >
                        允许本次加载
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}
