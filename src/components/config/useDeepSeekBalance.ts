import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface DeepSeekBalance {
    available: boolean;
    currency: string;
    total: string;
}

export type BalanceState =
    | { status: "idle" | "loading" }
    | { status: "done"; balance: DeepSeekBalance }
    | { status: "error"; message: string };

const CURRENCY_SYMBOLS: Record<string, string> = { CNY: "¥", USD: "$" };

/** 「¥19.00」；余额不可用（欠费或为 0）时附说明。 */
export function formatBalance(balance: DeepSeekBalance): string {
    const symbol = CURRENCY_SYMBOLS[balance.currency];
    const amount = symbol ? `${symbol}${balance.total}` : `${balance.total} ${balance.currency}`;
    return balance.available ? amount : `${amount}（余额不足，暂不可调用）`;
}

export function describeBalance(state: BalanceState): string {
    switch (state.status) {
        case "idle": return "";
        case "loading": return "查询中…";
        case "done": return formatBalance(state.balance);
        case "error": return `查询失败：${state.message}`;
    }
}

/**
 * 查询 DeepSeek 服务商余额（后端按服务商配置与保险箱确认发请求，Key 不经过前端）。
 * `providerId` 为空或 `enabled` 为 false 时不查询；`refreshKey` 变化时重新查询。
 */
export function useDeepSeekBalance(providerId: number | null, enabled: boolean, refreshKey: unknown = null) {
    const [state, setState] = useState<BalanceState>({ status: "idle" });
    const seqRef = useRef(0);

    const load = useCallback(async () => {
        if (providerId === null || !enabled) {
            setState({ status: "idle" });
            return;
        }
        const seq = ++seqRef.current;
        setState({ status: "loading" });
        try {
            const balance = await invoke<DeepSeekBalance>("get_deepseek_balance", { llmProviderId: providerId });
            if (seq !== seqRef.current) return;
            setState(balance && typeof balance.total === "string"
                ? { status: "done", balance }
                : { status: "error", message: "余额响应无效" });
        } catch (error) {
            if (seq === seqRef.current) setState({ status: "error", message: String(error) });
        }
    }, [providerId, enabled]);

    useEffect(() => {
        void load();
    }, [load, refreshKey]);

    return { state, reload: load };
}
