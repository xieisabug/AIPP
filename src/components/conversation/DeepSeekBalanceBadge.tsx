import React, { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Loader2, Wallet } from "lucide-react";
import {
    formatBalance,
    formatBalanceAmount,
    type BalanceState,
    type DeepSeekBalance,
} from "../config/useDeepSeekBalance";
import { cn } from "@/utils/utils";

interface CachedBalance {
    balance: DeepSeekBalance;
    updatedAt: number;
}

const BALANCE_CACHE_TTL_MS = 15_000;
const DEFAULT_SETTLE_DELAY_MS = 400;
const balanceCache = new Map<number, CachedBalance>();

export function clearDeepSeekBalanceCache() {
    balanceCache.clear();
}

export interface UseChatDeepSeekBalanceOptions {
    conversationId?: number | null;
    assistantId?: number | null;
    settleDelayMs?: number;
}

/**
 * 解析当前会话/助手是否绑定了开启「在聊天窗口展示余额」的 DeepSeek 供应商，
 * 并在一轮对话完全结束（runtime_state is_running: true -> false 且稳定空闲）时刷新余额。
 */
export function useChatDeepSeekBalance({
    conversationId,
    assistantId,
    settleDelayMs = DEFAULT_SETTLE_DELAY_MS,
}: UseChatDeepSeekBalanceOptions) {
    const normalizedConvId =
        typeof conversationId === "number" && Number.isFinite(conversationId) && conversationId > 0
            ? conversationId
            : null;
    const normalizedAssistantId =
        typeof assistantId === "number" && Number.isFinite(assistantId) && assistantId > 0
            ? assistantId
            : null;

    const [providerId, setProviderId] = useState<number | null>(null);
    const [state, setState] = useState<BalanceState>({ status: "idle" });
    const [refreshing, setRefreshing] = useState(false);

    const providerIdRef = useRef<number | null>(null);
    const resolveSeqRef = useRef(0);
    const fetchSeqRef = useRef(0);

    const fetchBalance = useCallback(async (targetProviderId: number, keepPrevious: boolean) => {
        const seq = ++fetchSeqRef.current;
        setRefreshing(true);
        if (!keepPrevious && !balanceCache.has(targetProviderId)) {
            setState({ status: "loading" });
        }
        try {
            const balance = await invoke<DeepSeekBalance>("get_deepseek_balance", {
                llmProviderId: targetProviderId,
            });
            if (seq !== fetchSeqRef.current || providerIdRef.current !== targetProviderId) return;
            if (balance && typeof balance.total === "string") {
                balanceCache.set(targetProviderId, { balance, updatedAt: Date.now() });
                setState({ status: "done", balance });
            } else {
                setState({ status: "error", message: "余额响应无效" });
            }
        } catch (error) {
            if (seq !== fetchSeqRef.current || providerIdRef.current !== targetProviderId) return;
            setState({ status: "error", message: String(error) });
        } finally {
            if (seq === fetchSeqRef.current) {
                setRefreshing(false);
            }
        }
    }, []);

    // 解析当前会话/助手是否需要展示 DeepSeek 余额，并监听供应商/助手/总管家模型配置变化
    useEffect(() => {
        let disposed = false;

        const resolveProvider = async (forceRefreshOnSameProvider: boolean) => {
            if (normalizedConvId === null && normalizedAssistantId === null) {
                providerIdRef.current = null;
                setProviderId(null);
                return;
            }
            const seq = ++resolveSeqRef.current;
            try {
                const next = await invoke<number | null>("get_deepseek_chat_balance_provider", {
                    conversationId: normalizedConvId,
                    assistantId: normalizedAssistantId,
                });
                if (disposed || seq !== resolveSeqRef.current) return;
                const resolved =
                    typeof next === "number" && Number.isFinite(next) && next > 0 ? next : null;
                const previous = providerIdRef.current;
                providerIdRef.current = resolved;
                setProviderId(resolved);
                if (resolved !== null && forceRefreshOnSameProvider && previous === resolved) {
                    void fetchBalance(resolved, true);
                }
            } catch {
                if (disposed || seq !== resolveSeqRef.current) return;
                providerIdRef.current = null;
                setProviderId(null);
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
    }, [normalizedConvId, normalizedAssistantId, fetchBalance]);

    // 切换或启用 DeepSeek 供应商时加载余额（命中近期缓存先立即展示，过期或首次则请求刷新）
    useEffect(() => {
        if (providerId === null) {
            fetchSeqRef.current += 1;
            setState({ status: "idle" });
            setRefreshing(false);
            return;
        }
        const cached = balanceCache.get(providerId);
        if (cached) {
            setState({ status: "done", balance: cached.balance });
            if (Date.now() - cached.updatedAt > BALANCE_CACHE_TTL_MS) {
                void fetchBalance(providerId, true);
            }
            return;
        }
        void fetchBalance(providerId, false);
    }, [providerId, fetchBalance]);

    // 监听当前会话运行态：在一轮对话完全结束（is_running: true -> false 且不再立即续跑）后更新余额
    useEffect(() => {
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
                        void fetchBalance(providerId, true);
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
    }, [providerId, normalizedConvId, settleDelayMs, fetchBalance]);

    const reload = useCallback(() => {
        if (providerId === null) return Promise.resolve();
        return fetchBalance(providerId, true);
    }, [providerId, fetchBalance]);

    return { providerId, state, refreshing, reload };
}

export interface DeepSeekBalanceBadgeProps {
    conversationId?: number | null;
    assistantId?: number | null;
    className?: string;
    settleDelayMs?: number;
}

const DeepSeekBalanceBadge: React.FC<DeepSeekBalanceBadgeProps> = React.memo(({
    conversationId,
    assistantId,
    className,
    settleDelayMs,
}) => {
    const { providerId, state, refreshing, reload } = useChatDeepSeekBalance({
        conversationId,
        assistantId,
        settleDelayMs,
    });

    if (providerId === null || state.status === "idle") {
        return null;
    }

    const unavailable = state.status === "done" && !state.balance.available;
    const isError = state.status === "error";

    let label = "";
    let tooltip = "";
    switch (state.status) {
        case "loading":
            label = "余额查询中…";
            tooltip = "正在查询 DeepSeek 账户余额…";
            break;
        case "done":
            label = state.balance.available
                ? formatBalanceAmount(state.balance)
                : `${formatBalanceAmount(state.balance)} 余额不足`;
            tooltip = `DeepSeek 账户余额：${formatBalance(state.balance)}（点击刷新）`;
            break;
        case "error":
            label = "余额异常";
            tooltip = `DeepSeek 余额查询失败：${state.message}（点击重试）`;
            break;
    }

    return (
        <button
            type="button"
            onClick={() => void reload()}
            disabled={refreshing}
            title={tooltip}
            aria-label={tooltip}
            data-aipp-slot="chat-deepseek-balance"
            data-testid="chat-deepseek-balance"
            className={cn(
                "inline-flex h-8 shrink-0 items-center gap-1.5 rounded-2xl border px-2.5 text-xs font-medium tabular-nums transition-colors",
                refreshing ? "cursor-wait" : "cursor-pointer",
                unavailable
                    ? "border-destructive/40 bg-destructive/5 text-destructive hover:border-destructive"
                    : isError
                        ? "border-border bg-muted/40 text-muted-foreground hover:border-primary hover:text-foreground"
                        : "border-secondary bg-primary-foreground text-muted-foreground hover:border-primary hover:text-foreground",
                className,
            )}
        >
            {refreshing ? (
                <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin" />
            ) : (
                <Wallet className="h-3.5 w-3.5 shrink-0" />
            )}
            <span className="whitespace-nowrap">{label}</span>
        </button>
    );
});

DeepSeekBalanceBadge.displayName = "DeepSeekBalanceBadge";

export default DeepSeekBalanceBadge;
