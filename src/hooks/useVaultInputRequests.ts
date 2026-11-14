import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { VaultInputRequest } from "@/data/Vault";
import { usePendingVaultRequests } from "./usePendingVaultRequests";

const CHANNELS = { list: "vault_list_input_requests", request: "vault-input-request", resolved: "vault-input-resolved" };

/**
 * 助手请用户填写保险箱里还没有的值。值经 vault_submit_input 直接存进保险箱，
 * 不经过 ask-user 的答案通道（那条通道会把答案交给助手）。
 */
export function useVaultInputRequests(conversationIds: number | number[] | null | undefined) {
    const [requests] = usePendingVaultRequests<VaultInputRequest>(conversationIds, CHANNELS);

    /** 失败时抛出，卡片保持打开以便用户修改后重试。 */
    const submit = useCallback(async (requestId: string, values: Record<string, string>, sync: boolean) => {
        await invoke("vault_submit_input", { requestId, values, sync });
    }, []);

    const decline = useCallback(async (requestId: string) => {
        await invoke("vault_decline_input", { requestId });
    }, []);

    return { requests, submit, decline };
}
