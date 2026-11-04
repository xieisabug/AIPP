import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useDataRefresh } from "@/hooks/useDataRefresh";
import { useMobileResume } from "@/hooks/useMobileResume";

export interface RecoverySnapshot {
    conversation_id: number;
    local_running: boolean;
    unknown_tool_ids: number[];
    incomplete_message_ids: number[];
    missing_attachment_ids: number[];
}

export function recoveryNotices(snapshot: RecoverySnapshot): string[] {
    const notices: string[] = [];
    if (snapshot.unknown_tool_ids.length) notices.push(`工具 ${snapshot.unknown_tool_ids.join("、")} 的记录显示执行中，但未观察到本机执行实例，结果未知。不会自动重试。`);
    if (snapshot.incomplete_message_ids.length) notices.push(`消息 ${snapshot.incomplete_message_ids.join("、")} 没有完成记录，当前本机未运行。已保留收到的内容，不会自动重发。`);
    if (snapshot.missing_attachment_ids.length) notices.push(`附件 ${snapshot.missing_attachment_ids.join("、")} 的内容在此设备不可用，请重新导入。同步历史不代表附件实体已到达。`);
    return notices;
}

export default function MobileRecoveryNotice({ conversationId }: { conversationId: string }) {
    const [result, setResult] = useState<{ id: string; notices: string[]; error?: string }>();
    const refresh = useDataRefresh({
        events: ["conversation_data_changed"],
        identity: conversationId,
        initial: true,
        refresh: async (isCurrent) => {
            if (!conversationId) return;
            const snapshot = await invoke<RecoverySnapshot>("get_mobile_recovery_snapshot", { conversationId: Number(conversationId) });
            if (isCurrent()) setResult({ id: conversationId, notices: recoveryNotices(snapshot) });
        },
        onError: error => setResult({ id: conversationId, notices: [], error: `恢复状态读取失败（conversation=${conversationId}）: ${String(error)}` }),
    });
    useMobileResume(refresh, conversationId);
    if (!result || result.id !== conversationId || (!result.error && !result.notices.length)) return null;
    return <div role="status" className="shrink-0 border-b border-border bg-muted px-3 py-2 text-xs text-muted-foreground max-h-32 overflow-y-auto">
        {result.error ? <p>{result.error}</p> : result.notices.map(notice => <p key={notice}>{notice}</p>)}
        <button type="button" className="min-h-11 underline" onClick={refresh}>刷新本机状态</button>
    </div>;
}
