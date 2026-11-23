import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, ShieldCheck } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    CompactDialogFooter,
} from "@/components/ui/dialog";
import { cn } from "@/utils/utils";
import { getErrorMessage } from "@/utils/error";
import {
    describePreviewResourceOrigin,
    getActionablePreviewResources,
    getBlockedPreviewResources,
    getDefaultSelectedPreviewResourceIds,
    previewResourceHost,
    previewResourceTypeLabel,
    PreviewExternalResource,
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

type FeatureConfigRow = { feature_code: string; key: string; value: string };

/** 是否配置了网络代理（设置 → 网络配置）；读取失败时返回 null，不阻止用户尝试。 */
async function readProxyConfigured(): Promise<boolean | null> {
    const rows = await invoke<FeatureConfigRow[]>("get_all_feature_config");
    if (!Array.isArray(rows)) return null;
    return rows.some(
        (row) => row.feature_code === "network_config" && row.key === "network_proxy" && row.value.trim() !== ""
    );
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

/** 完整地址；原写法不同（例如样式表里的相对路径）时放在悬浮提示里。 */
function ResourceUrl({ resource }: { resource: PreviewExternalResource }) {
    const differs = resource.originalUrl !== resource.normalizedUrl;
    return (
        <div
            className="whitespace-pre-wrap break-all rounded border border-border bg-muted/40 px-2 py-1 font-mono text-xs text-foreground"
            title={differs ? `原写法：${resource.originalUrl}` : undefined}
        >
            {resource.normalizedUrl || resource.originalUrl}
        </div>
    );
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
    const [proxyConfigured, setProxyConfigured] = useState<boolean | null>(null);

    useEffect(() => {
        if (!open) {
            setError(null);
            setLocalResources(null);
            return;
        }
        setLocalResources(null);
        setSelectedIds(getDefaultSelectedPreviewResourceIds(externalResources));
    }, [externalResources, open]);

    useEffect(() => {
        if (!open) return;
        let active = true;
        readProxyConfigured()
            .then((configured) => {
                if (active) setProxyConfigured(configured);
            })
            .catch(() => {
                if (active) setProxyConfigured(null);
            });
        return () => {
            active = false;
        };
    }, [open]);

    const effectiveResources = localResources ?? externalResources ?? null;
    const resources = getActionablePreviewResources(effectiveResources);
    const blocked = getBlockedPreviewResources(effectiveResources);
    const visibleError = error ?? scanError;
    const selectableIds = useMemo(() => new Set(resources.map((resource) => resource.id)), [resources]);
    const selectedResourceIds = Array.from(selectedIds).filter((id) => selectableIds.has(id));
    const selectedCount = selectedResourceIds.length;
    const authorizeDisabled = selectedCount === 0 || isSubmitting || !canAuthorize;

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
                        ? "部分资源加载失败，原因见各项说明。"
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
        <Dialog open={open} onOpenChange={(next) => { if (!isSubmitting) onOpenChange(next); }}>
            <CompactDialogContent className="sm:max-w-3xl">
                <CompactDialogHeader
                    icon={<ShieldCheck className="h-4 w-4" />}
                    title="外部资源授权"
                    subtitle={resources.length > 0 ? `${resources.length} 项待处理` : undefined}
                    description="预览引用了外部资源，默认不加载。勾选的资源由本机下载后提供给预览。"
                    closeDisabled={isSubmitting}
                />

                <CompactDialogBody className="flex flex-col gap-3 overflow-hidden">
                    <p className="shrink-0 text-xs text-muted-foreground">
                        预览引用了下列外部资源，默认不加载。勾选的资源由本机下载后提供给预览；允许样式表时，会一并加载它引用的字体和图片。
                    </p>

                    {visibleError && (
                        <div className="shrink-0 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-xs text-destructive">
                            {visibleError}
                        </div>
                    )}

                    {isLoading && resources.length === 0 ? (
                        <div className="rounded-lg border border-border bg-muted/30 px-3 py-6 text-center text-sm text-muted-foreground">
                            正在检测外部资源...
                        </div>
                    ) : resources.length === 0 && blocked.length === 0 ? (
                        <div className="rounded-lg border border-dashed border-border bg-muted/20 px-3 py-6 text-center text-sm text-muted-foreground">
                            当前预览没有待处理的外部资源。
                        </div>
                    ) : (
                        <div className="min-h-0 max-h-[54vh] flex-1 space-y-2.5 overflow-y-auto pr-0.5">
                            {resources.map((resource) => {
                                const checked = selectedIds.has(resource.id);
                                return (
                                    <label
                                        key={resource.id}
                                        className={cn(
                                            "flex cursor-pointer gap-3 rounded-lg border border-border p-3 transition-colors hover:bg-muted/50",
                                            checked && "border-foreground/40 bg-muted/50",
                                        )}
                                    >
                                        <Checkbox
                                            className="mt-1"
                                            checked={checked}
                                            disabled={isSubmitting}
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
                                                <Badge variant="outline">{previewResourceTypeLabel(resource.type)}</Badge>
                                                <Badge
                                                    variant={resource.risk === "high" ? "destructive" : "secondary"}
                                                    className="gap-1"
                                                >
                                                    {resource.risk === "high" && (
                                                        <AlertTriangle className="h-3 w-3" />
                                                    )}
                                                    {riskLabel[resource.risk]}
                                                </Badge>
                                                {resource.status === "failed" && (
                                                    <Badge variant="outline">加载失败</Badge>
                                                )}
                                                <span className="text-xs text-muted-foreground">
                                                    {previewResourceHost(resource.normalizedUrl)}
                                                </span>
                                            </div>
                                            <ResourceUrl resource={resource} />
                                            <div className="text-xs text-muted-foreground">
                                                {describePreviewResourceOrigin(resource)}
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
                            {blocked.length > 0 && (
                                <section className="space-y-2" aria-label="无法加载的地址">
                                    <div className="text-xs text-muted-foreground">
                                        以下地址无法解析（相对路径缺少基准地址），不能授权加载：
                                    </div>
                                    {blocked.map((resource) => (
                                        <div
                                            key={resource.id}
                                            className="space-y-1 rounded-lg border border-dashed border-border p-3"
                                        >
                                            <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                                                <Badge variant="outline">{previewResourceTypeLabel(resource.type)}</Badge>
                                                {describePreviewResourceOrigin(resource)}
                                            </div>
                                            <ResourceUrl resource={resource} />
                                        </div>
                                    ))}
                                </section>
                            )}
                        </div>
                    )}
                </CompactDialogBody>

                <CompactDialogFooter>
                    {proxyConfigured === false && resources.length > 0 && (
                        <span className="mr-auto text-xs text-muted-foreground">
                            未配置网络代理（设置 → 网络配置）
                        </span>
                    )}
                    {onRetryScan && resources.length === 0 && (
                        <Button
                            type="button"
                            variant="outline"
                            className="min-h-11 px-3 sm:min-h-9"
                            onClick={onRetryScan}
                            disabled={isSubmitting || isLoading}
                        >
                            重新检测
                        </Button>
                    )}
                    <Button
                        type="button"
                        variant="outline"
                        className="min-h-11 px-3 sm:min-h-9"
                        onClick={() => onOpenChange(false)}
                        disabled={isSubmitting}
                    >
                        取消
                    </Button>
                    <Button
                        type="button"
                        variant="outline"
                        className="min-h-11 px-3 sm:min-h-9"
                        disabled={authorizeDisabled}
                        onClick={() => void submit(true)}
                    >
                        加入白名单并加载
                    </Button>
                    <Button
                        type="button"
                        variant="outline"
                        className="min-h-11 px-3 sm:min-h-9"
                        disabled={authorizeDisabled || proxyConfigured === false}
                        onClick={() => void submit(false, true)}
                    >
                        使用代理加载所选资源
                    </Button>
                    <Button
                        type="button"
                        className="min-h-11 px-4 sm:min-h-9"
                        disabled={authorizeDisabled}
                        onClick={() => void submit(false)}
                    >
                        允许本次加载
                    </Button>
                </CompactDialogFooter>
            </CompactDialogContent>
        </Dialog>
    );
}
