import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Loader2, RefreshCw, Wallet } from "lucide-react";
import { HoverCard, HoverCardContent, HoverCardTrigger } from "@/components/ui/hover-card";
import {
    classifyQuotaDisplay,
    QuotaDetailsList,
    QuotaRingsChart,
    type QuotaReading,
} from "../config/quotaVisuals";
import { cn } from "@/utils/utils";

export interface ChatQuotaProvider {
    providerId: number;
    providerName: string;
    apiType: string;
    label: string;
}

export type ChatQuotaState =
    | { status: "idle" | "loading" }
    | { status: "done"; reading: QuotaReading; error?: string }
    | { status: "error"; message: string };

interface CachedQuota {
    reading: QuotaReading;
    updatedAt: number;
}

const QUOTA_MEMORY_CACHE_TTL_MS = 15_000;
const DEFAULT_SETTLE_DELAY_MS = 400;
const quotaCache = new Map<number, CachedQuota>();

export function clearProviderQuotaCache() {
    quotaCache.clear();
}

export interface UseChatProviderQuotaOptions {
    conversationId?: number | null;
    assistantId?: number | null;
    settleDelayMs?: number;
}

/**
 * 解析当前会话/助手是否绑定了开启「聊天窗口展示」的订阅额度供应商，
 * 支持点击手动刷新与一轮对话完全结束（runtime_state is_running: true -> false 且稳定空闲）后自动刷新。
 */
export function useChatProviderQuota({
    conversationId,
    assistantId,
    settleDelayMs = DEFAULT_SETTLE_DELAY_MS,
}: UseChatProviderQuotaOptions) {
    const normalizedConvId =
        typeof conversationId === "number" && Number.isFinite(conversationId) && conversationId > 0
            ? conversationId
            : null;
    const normalizedAssistantId =
        typeof assistantId === "number" && Number.isFinite(assistantId) && assistantId > 0
            ? assistantId
            : null;

    const [provider, setProvider] = useState<ChatQuotaProvider | null>(null);
    const [state, setState] = useState<ChatQuotaState>({ status: "idle" });
    const [refreshing, setRefreshing] = useState(false);

    const providerIdRef = useRef<number | null>(null);
    const resolveSeqRef = useRef(0);
    const fetchSeqRef = useRef(0);

    const fetchQuota = useCallback(async (targetProviderId: number, force: boolean, keepPrevious: boolean) => {
        const seq = ++fetchSeqRef.current;
        setRefreshing(true);
        if (!keepPrevious && !quotaCache.has(targetProviderId)) {
            setState({ status: "loading" });
        }
        try {
            const reading = await invoke<QuotaReading>("get_provider_quota", {
                providerId: targetProviderId,
                force,
            });
            if (seq !== fetchSeqRef.current || providerIdRef.current !== targetProviderId) return;
            if (reading && Array.isArray(reading.windows)) {
                quotaCache.set(targetProviderId, { reading, updatedAt: Date.now() });
                setState({ status: "done", reading });
            } else {
                const cached = quotaCache.get(targetProviderId);
                setState(
                    cached
                        ? { status: "done", reading: cached.reading, error: "额度响应无效" }
                        : { status: "error", message: "额度响应无效" },
                );
            }
        } catch (error) {
            if (seq !== fetchSeqRef.current || providerIdRef.current !== targetProviderId) return;
            const message = String(error);
            const cached = quotaCache.get(targetProviderId);
            setState(
                cached
                    ? { status: "done", reading: cached.reading, error: message }
                    : { status: "error", message },
            );
        } finally {
            if (seq === fetchSeqRef.current) {
                setRefreshing(false);
            }
        }
    }, []);

    // 解析当前会话/助手是否需要展示订阅额度，并监听供应商/助手/总管家模型配置变化
    useEffect(() => {
        let disposed = false;

        const resolveProvider = async (refreshSameProvider: boolean) => {
            if (normalizedConvId === null && normalizedAssistantId === null) {
                providerIdRef.current = null;
                setProvider(null);
                return;
            }
            const seq = ++resolveSeqRef.current;
            try {
                const next = await invoke<ChatQuotaProvider | null>("get_chat_quota_provider", {
                    conversationId: normalizedConvId,
                    assistantId: normalizedAssistantId,
                });
                if (disposed || seq !== resolveSeqRef.current) return;
                const resolved =
                    next && typeof next.providerId === "number" && Number.isFinite(next.providerId) && next.providerId > 0
                        ? next
                        : null;
                const previousId = providerIdRef.current;
                providerIdRef.current = resolved?.providerId ?? null;
                setProvider(resolved);
                if (resolved && refreshSameProvider && previousId === resolved.providerId) {
                    void fetchQuota(resolved.providerId, false, true);
                }
            } catch {
                if (disposed || seq !== resolveSeqRef.current) return;
                providerIdRef.current = null;
                setProvider(null);
            }
        };

        void resolveProvider(false);

        const unlistenPromises = [
            listen("llm_provider_changed", () => void resolveProvider(true)),
            listen("assistant_list_changed", () => void resolveProvider(false)),
            listen("feature_config_changed", () => void resolveProvider(false)),
        ];

        return () => {
            disposed = true;
            resolveSeqRef.current += 1;
            for (const promise of unlistenPromises) {
                void promise.then((stop) => stop()).catch(() => {});
            }
        };
    }, [normalizedConvId, normalizedAssistantId, fetchQuota]);

    // 切换或启用支持订阅额度的供应商时加载额度（命中近期缓存先立即展示，否则请求后端缓存/远端）
    useEffect(() => {
        const providerId = provider?.providerId ?? null;
        if (providerId === null) {
            fetchSeqRef.current += 1;
            setState({ status: "idle" });
            setRefreshing(false);
            return;
        }
        const cached = quotaCache.get(providerId);
        if (cached) {
            setState({ status: "done", reading: cached.reading });
            if (Date.now() - cached.updatedAt > QUOTA_MEMORY_CACHE_TTL_MS) {
                void fetchQuota(providerId, false, true);
            }
        } else {
            void fetchQuota(providerId, false, false);
        }

        const unlistenCursor = listen(`cursor-account-changed-${providerId}`, () => {
            quotaCache.delete(providerId);
            void fetchQuota(providerId, true, false);
        });
        return () => {
            void unlistenCursor.then((stop) => stop()).catch(() => {});
        };
    }, [provider?.providerId, fetchQuota]);

    // 监听当前会话运行态：在一轮对话完全结束（is_running: true -> false 且稳定空闲）后强制刷新额度
    useEffect(() => {
        const providerId = provider?.providerId ?? null;
        if (providerId === null || normalizedConvId === null) return;

        let disposed = false;
        let wasRunning = false;
        let sawSnapshot = false;
        let settleTimer: ReturnType<typeof setTimeout> | null = null;

        const clearSettleTimer = () => {
            if (settleTimer !== null) {
                clearTimeout(settleTimer);
                settleTimer = null;
            }
        };

        const handleRunningChange = (isRunning: boolean) => {
            if (isRunning) {
                wasRunning = true;
                clearSettleTimer();
                return;
            }
            if (wasRunning) {
                wasRunning = false;
                clearSettleTimer();
                settleTimer = setTimeout(() => {
                    settleTimer = null;
                    if (!disposed) {
                        void fetchQuota(providerId, true, true);
                    }
                }, settleDelayMs);
            }
        };

        void invoke<{ is_running?: boolean }>("get_conversation_runtime_state", {
            conversationId: normalizedConvId,
        })
            .then((runtime) => {
                if (!disposed && !sawSnapshot && runtime?.is_running === true) {
                    wasRunning = true;
                }
            })
            .catch(() => {});

        const unlistenPromise = listen<{
            type?: string;
            data?: { state?: { is_running?: boolean } };
        }>(`conversation_event_${normalizedConvId}`, (event) => {
            if (disposed || event.payload?.type !== "runtime_state_snapshot") return;
            const isRunning = event.payload.data?.state?.is_running;
            if (typeof isRunning !== "boolean") return;
            sawSnapshot = true;
            handleRunningChange(isRunning);
        });

        return () => {
            disposed = true;
            clearSettleTimer();
            void unlistenPromise.then((stop) => stop()).catch(() => {});
        };
    }, [provider?.providerId, normalizedConvId, settleDelayMs, fetchQuota]);

    const reload = useCallback(() => {
        const providerId = provider?.providerId ?? null;
        if (providerId === null) return Promise.resolve();
        return fetchQuota(providerId, true, true);
    }, [provider?.providerId, fetchQuota]);

    return { provider, state, refreshing, reload };
}

export interface ProviderQuotaBadgeProps {
    conversationId?: number | null;
    assistantId?: number | null;
    className?: string;
    settleDelayMs?: number;
}

const ProviderQuotaBadge: React.FC<ProviderQuotaBadgeProps> = React.memo(({
    conversationId,
    assistantId,
    className,
    settleDelayMs,
}) => {
    const { provider, state, refreshing, reload } = useChatProviderQuota({
        conversationId,
        assistantId,
        settleDelayMs,
    });

    const reading = state.status === "done" ? state.reading : null;
    const errorMessage =
        state.status === "error"
            ? state.message
            : state.status === "done"
                ? state.error
                : undefined;

    const display = useMemo(
        () => (reading ? classifyQuotaDisplay(reading.windows) : null),
        [reading],
    );

    if (!provider || state.status === "idle") {
        return null;
    }

    const isError = state.status === "error";
    const isDepleted = Boolean(display?.hasDepleted);
    const isRings = display?.kind === "rings";
    const isAmount = display?.kind === "amount";

    let ariaLabel = `${provider.providerName} ${provider.label}（点击刷新）`;
    if (state.status === "loading") {
        ariaLabel = `正在查询 ${provider.providerName} ${provider.label}…`;
    } else if (isError) {
        ariaLabel = `${provider.providerName} ${provider.label}查询失败：${errorMessage}（点击重试）`;
    } else if (isAmount && display) {
        ariaLabel = `${provider.providerName} ${provider.label}：${display.formattedAmount}（点击刷新）`;
    }

    return (
        <HoverCard openDelay={150} closeDelay={120}>
            <HoverCardTrigger asChild>
                <button
                    type="button"
                    onClick={() => {
                        if (!refreshing) void reload();
                    }}
                    aria-busy={refreshing}
                    aria-label={ariaLabel}
                    data-aipp-slot="chat-provider-quota"
                    data-testid="chat-provider-quota"
                    data-quota-mode={display?.kind ?? state.status}
                    className={cn(
                        "inline-flex h-8 shrink-0 items-center gap-1.5 rounded-2xl border text-xs font-medium tabular-nums transition-colors",
                        isRings ? "px-1.5" : "px-2.5",
                        refreshing ? "cursor-wait" : "cursor-pointer",
                        isDepleted
                            ? "border-destructive/40 bg-destructive/5 text-destructive hover:border-destructive"
                            : isError
                                ? "border-border bg-muted/40 text-muted-foreground hover:border-primary hover:text-foreground"
                                : "border-secondary bg-primary-foreground text-muted-foreground hover:border-primary hover:text-foreground",
                        className,
                    )}
                >
                    {isRings && display ? (
                        <>
                            <QuotaRingsChart
                                clusters={display.clusters}
                                size={22}
                                className={refreshing ? "opacity-65" : undefined}
                            />
                            {refreshing && (
                                <Loader2 className="h-3 w-3 shrink-0 animate-spin text-muted-foreground" />
                            )}
                        </>
                    ) : isAmount && display ? (
                        <>
                            {refreshing ? (
                                <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin" />
                            ) : (
                                <Wallet className="h-3.5 w-3.5 shrink-0" />
                            )}
                            <span className="whitespace-nowrap">{display.formattedAmount}</span>
                        </>
                    ) : state.status === "loading" ? (
                        <>
                            <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin" />
                            <span className="whitespace-nowrap">额度查询中…</span>
                        </>
                    ) : (
                        <>
                            <RefreshCw className="h-3.5 w-3.5 shrink-0" />
                            <span className="whitespace-nowrap">额度异常</span>
                        </>
                    )}
                </button>
            </HoverCardTrigger>
            <HoverCardContent
                align="end"
                sideOffset={8}
                className="w-72 rounded-xl border-border p-3.5 shadow-lg"
                data-testid="chat-provider-quota-popover"
            >
                <div className="space-y-3">
                    <div className="flex items-center justify-between gap-2 border-b border-border/70 pb-2">
                        <div className="min-w-0">
                            <div className="truncate text-xs font-semibold text-foreground">
                                {provider.providerName}
                            </div>
                            <div className="text-[11px] text-muted-foreground">{provider.label}</div>
                        </div>
                        <span className="shrink-0 text-[11px] text-muted-foreground">
                            {refreshing
                                ? "刷新中…"
                                : reading
                                    ? `更新于 ${new Date(reading.checkedAt).toLocaleTimeString()}`
                                    : ""}
                        </span>
                    </div>

                    {errorMessage && (
                        <p
                            role="alert"
                            className="whitespace-pre-wrap break-words rounded-md bg-destructive/10 px-2.5 py-1.5 text-xs text-destructive"
                        >
                            刷新额度失败：{errorMessage}
                        </p>
                    )}

                    {reading && display && reading.windows.length > 0 ? (
                        <QuotaDetailsList
                            windows={reading.windows}
                            colorByIndex={display.colorByIndex}
                            compact
                        />
                    ) : state.status === "loading" ? (
                        <p className="text-xs text-muted-foreground">正在查询额度…</p>
                    ) : null}

                    <p className="border-t border-border/60 pt-2 text-[11px] text-muted-foreground">
                        点击图标刷新 · 会话结束后自动更新
                    </p>
                </div>
            </HoverCardContent>
        </HoverCard>
    );
});

ProviderQuotaBadge.displayName = "ProviderQuotaBadge";

export default ProviderQuotaBadge;
