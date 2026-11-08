import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import type { AssistantDetail, AssistantListItem } from "@/data/Assistant";
import type { AssistantItemAction } from "@/components/config/assistant/AssistantItemContextMenu";
import EditAssistantDialog from "@/components/config/EditAssistantDialog";
import ShareDialog from "@/components/ShareDialog";
import ConfirmDialog from "@/components/ConfirmDialog";

export function useAssistantItemActions({ onCopied, onClear }: {
    onCopied?: (assistant: AssistantDetail) => void | Promise<void>;
    onClear?: (assistant: AssistantListItem) => Promise<void>;
} = {}) {
    const [editing, setEditing] = useState<AssistantDetail | null>(null);
    const [shareCode, setShareCode] = useState<string | null>(null);
    const [confirmation, setConfirmation] = useState<{ assistant: AssistantListItem; action: "delete" | "clear" } | null>(null);
    const busy = useRef(false);
    const request = useRef(0);
    const run = async (assistant: AssistantListItem, action: AssistantItemAction) => {
        if (busy.current) return;
        if (assistant.id === 1 && (action === "delete" || action === "copy")) return;
        if (action === "delete" || action === "clear") { setConfirmation({ assistant, action }); return; }
        const ticket = ++request.current;
        busy.current = true;
        try {
            if (action === "edit") {
                const detail = await invoke<AssistantDetail>("get_assistant", { assistantId: assistant.id });
                if (ticket === request.current) setEditing(detail);
            } else if (action === "copy") {
                const copied = await invoke<AssistantDetail>("copy_assistant", { assistantId: assistant.id });
                toast.success("复制助手成功");
                await onCopied?.(copied);
            } else {
                const code = await invoke<string>("export_assistant", { assistantId: assistant.id });
                if (ticket === request.current) setShareCode(code);
            }
        } catch (error) { toast.error(`${action === "edit" ? "读取助手编辑信息" : action === "copy" ? "复制助手" : "分享助手"}失败（${assistant.name}，${assistant.id}）：${String(error)}`); }
        finally { busy.current = false; }
    };
    const confirm = async () => {
        if (!confirmation || busy.current) return;
        busy.current = true;
        const { assistant, action } = confirmation;
        try {
            if (action === "clear") {
                if (!onClear) throw new Error("当前界面不支持清空消息历史");
                await onClear(assistant);
            } else await invoke("delete_assistant", { assistantId: assistant.id });
            setConfirmation(null);
            toast.success(action === "clear" ? "消息历史已清空" : "删除助手成功");
        } catch (error) { toast.error(`${action === "clear" ? "清空消息历史" : "删除助手"}失败（助手 ${assistant.id}）：${String(error)}`); }
        finally { busy.current = false; }
    };
    const dialogs = <>
        <EditAssistantDialog isOpen={!!editing} currentAssistant={editing} onClose={() => setEditing(null)}
            onSave={detail => invoke("save_assistant", { assistantDetail: detail })} onAssistantUpdated={setEditing} />
        <ShareDialog title="助手配置" shareCode={shareCode ?? ""} isOpen={shareCode !== null} onClose={() => setShareCode(null)} />
        <ConfirmDialog isOpen={!!confirmation} title={confirmation?.action === "clear" ? "清空消息历史" : "删除助手"}
            confirmText={confirmation?.action === "clear"
                ? `确认清空「${confirmation.assistant.name}」在管家中的私聊历史？当前执行会停止，此操作不可恢复，不影响其他会话。`
                : `确认删除「${confirmation?.assistant.name}」？助手配置将被删除，已有对话转移到快速使用助手，此操作不可恢复。`}
            onConfirm={() => { void confirm(); }} onCancel={() => { if (!busy.current) setConfirmation(null); }} />
    </>;
    return { run, dialogs };
}
