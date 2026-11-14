import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import type { VaultUseDecision, VaultUseRequest } from "@/data/Vault";
import { usePendingVaultRequests } from "./usePendingVaultRequests";

const CHANNELS = { list: "vault_list_use_requests", request: "vault-use-request", resolved: "vault-use-resolved" };

/** 助手在工具调用里用了保险箱的占位符，等待用户批准这一次使用。 */
export function useVaultUseRequests(conversationIds: number | number[] | null | undefined) {
    const [requests, setRequests] = usePendingVaultRequests<VaultUseRequest>(conversationIds, CHANNELS);

    const respond = useCallback(async (requestId: string, decision: VaultUseDecision) => {
        try {
            await invoke("vault_respond_use_request", { requestId, decision });
        } catch (error) {
            toast.error(`处理保险箱确认失败：${String(error)}`);
            // 已经失效的请求不再显示。
            setRequests((current) => current.filter((r) => r.requestId !== requestId));
        }
    }, [setRequests]);

    return { requests, respond };
}
