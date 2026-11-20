import React, { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { Check, Loader2, RefreshCw } from "lucide-react";
import { Button } from "../ui/button";
import type { MCPServer } from "@/data/MCP";
import type { VaultItem } from "@/data/Vault";
import { useDataRefresh } from "@/hooks/useDataRefresh";
import { connectDeepSeekSearch, deepSeekSearchStatus, hideKey, type DeepSeekSearchStatus } from "./deepseekSuite";
import { describeBalance, useDeepSeekBalance } from "./useDeepSeekBalance";

interface DeepSeekCapabilitiesProps {
    providerId: number;
    providerName: string;
    /** 表单里当前的 API Key（明文或保险箱引用）。 */
    apiKey: string;
    endpoint: string;
    vault: { available: boolean; items: VaultItem[] };
    /** Key 被存入保险箱后，表单改显示引用。 */
    onKeyShared: (value: string) => void;
}

function describe(status: DeepSeekSearchStatus | null, hasKey: boolean): { text: string; action: string | null } {
    if (!hasKey) return { text: "填写 API Key 后可用于内置搜索", action: null };
    if (!status) return { text: "读取中…", action: null };
    switch (status.kind) {
        case "connected":
            if (!status.enabled) return { text: "内置搜索已停用", action: "启用" };
            return { text: status.fetch ? "联网搜索与网页抓取使用此 Key" : "联网搜索使用此 Key，网页抓取为本机", action: status.fetch ? null : "抓取也用 DeepSeek" };
        case "other_key":
            return { text: "内置搜索使用另一把 DeepSeek Key", action: "改用此 Key" };
        case "other_provider":
            return { text: `内置搜索当前使用 ${status.label}`, action: "改用 DeepSeek" };
        case "none":
            return { text: "未启用 · 每千次成功搜索 5 元", action: "启用" };
    }
}

/**
 * DeepSeek 服务商详情页的「能力」区块：展示这把 Key 被哪些能力共用，并可一键接入。
 * 只做总览与接入，具体参数在「应用与工具 → 搜索工具」里改，不在这里重复表单。
 */
const DeepSeekCapabilities: React.FC<DeepSeekCapabilitiesProps> = ({ providerId, providerName, apiKey, endpoint, vault, onKeyShared }) => {
    const [servers, setServers] = useState<MCPServer[] | null>(null);
    const [busy, setBusy] = useState(false);
    // Key 改动后等保存落库再查余额，避免逐字查询
    const [balanceKey, setBalanceKey] = useState(apiKey);
    useEffect(() => {
        const timer = setTimeout(() => setBalanceKey(apiKey), 800);
        return () => clearTimeout(timer);
    }, [apiKey]);
    const balance = useDeepSeekBalance(providerId, !!balanceKey.trim(), balanceKey);

    useDataRefresh({
        events: ["mcp_state_changed"],
        identity: providerId,
        initial: true,
        refresh: async (isCurrent) => {
            const next = await invoke<MCPServer[]>("get_mcp_servers");
            if (isCurrent()) setServers(next);
        },
    });

    const hasKey = !!apiKey.trim();
    const status = servers ? deepSeekSearchStatus(servers, apiKey) : null;
    const { text, action } = describe(status, hasKey);
    const connected = status?.kind === "connected" && status.enabled;

    const connect = useCallback(async () => {
        setBusy(true);
        try {
            const key = await connectDeepSeekSearch({ providerId, providerName, apiKey, endpoint, vault });
            if (key !== apiKey.trim()) onKeyShared(key);
            setServers(await invoke<MCPServer[]>("get_mcp_servers"));
            toast.success("内置搜索已改用 DeepSeek");
        } catch (error) {
            toast.error(`接入 DeepSeek 搜索失败: ${hideKey(String(error), apiKey)}`);
        } finally {
            setBusy(false);
        }
    }, [providerId, providerName, apiKey, endpoint, vault, onKeyShared]);

    return (
        <div className="divide-y divide-border rounded-lg border border-border" data-aipp-slot="deepseek-capabilities">
            {hasKey && (
                <div className="flex min-h-11 items-center gap-3 px-3 py-2">
                    <div className="min-w-0 flex-1">
                        <div className="text-sm font-medium">账户余额</div>
                        <div className="truncate text-xs text-muted-foreground" data-testid="deepseek-balance">
                            {describeBalance(balance.state)}
                        </div>
                    </div>
                    <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="h-7 w-7 shrink-0 p-0 text-muted-foreground"
                        aria-label="刷新余额"
                        disabled={balance.state.status === "loading"}
                        onClick={() => void balance.reload()}
                    >
                        <RefreshCw className={balance.state.status === "loading" ? "h-3.5 w-3.5 animate-spin" : "h-3.5 w-3.5"} />
                    </Button>
                </div>
            )}
            <div className="flex min-h-11 items-center gap-3 px-3 py-2">
                <div className="min-w-0 flex-1">
                    <div className="text-sm font-medium">搜索工具</div>
                    <div className="truncate text-xs text-muted-foreground" data-testid="deepseek-search-status">{text}</div>
                </div>
                {connected && !action && <Check className="h-4 w-4 shrink-0 text-muted-foreground" aria-label="已启用" />}
                {action && (
                    <Button type="button" variant="outline" size="sm" className="h-7 shrink-0 px-2.5 text-xs" disabled={busy} onClick={() => void connect()}>
                        {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                        {action}
                    </Button>
                )}
            </div>
        </div>
    );
};

export default DeepSeekCapabilities;
