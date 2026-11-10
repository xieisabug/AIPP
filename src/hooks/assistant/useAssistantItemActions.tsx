import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import type { AssistantDetail, AssistantListItem } from "@/data/Assistant";
import type { AssistantItemAction } from "@/components/config/assistant/AssistantItemContextMenu";
import EditAssistantDialog from "@/components/config/EditAssistantDialog";
import ShareDialog from "@/components/ShareDialog";
import ConfirmDialog from "@/components/ConfirmDialog";

export function useAssistantItemActions({ onCopied, onReset }: {
    onCopied?: (assistant: AssistantDetail) => void | Promise<void>;
    /** 重开该助手在管家中的私聊（旧私聊归档），仅管家联系人列表提供。 */
    onReset?: (assistant: AssistantListItem) => Promise<void>;
} = {}) {
    const [editing, setEditing] = useState<AssistantDetail | null>(null);
    const [shareCode, setShareCode] = useState<string | null>(null);
    const [confirmation, setConfirmation] = useState<{ assistant: AssistantListItem; action: "delete" | "reset" } | null>(null);
    const busy = useRef(false);
    const request = useRef(0);
    const run = async (assistant: AssistantListItem, action: AssistantItemAction) => {
        if (busy.current) return;
        if (assistant.id === 1 && (action === "delete" || action === "copy")) return;
        if (action === "delete" || action === "reset") { setConfirmation({ assistant, action }); return; }
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
            if (action === "reset") {
                if (!onReset) throw new Error("当前界面不支持重开私聊");
                await onReset(assistant);
            } else await invoke("delete_assistant", { assistantId: assistant.id });
            setConfirmation(null);
            toast.success(action === "reset" ? "旧私聊已归档，已开始新会话" : "删除助手成功");
        } catch (error) { toast.error(`${action === "reset" ? "重开新会话" : "删除助手"}失败（助手 ${assistant.id}）：${String(error)}`); }
        finally { busy.current = false; }
    };
    const dialogs = <>
        <EditAssistantDialog isOpen={!!editing} currentAssistant={editing} onClose={() => setEditing(null)}
            onSave={detail => invoke("save_assistant", { assistantDetail: detail })} onAssistantUpdated={setEditing} />
        <ShareDialog title="助手配置" shareCode={shareCode ?? ""} isOpen={shareCode !== null} onClose={() => setShareCode(null)} />
        <ConfirmDialog isOpen={!!confirmation} title={confirmation?.action === "reset" ? "重开新会话" : "删除助手"}
            confirmText={confirmation?.action === "reset"
                ? `确认重开与「${confirmation.assistant.name}」的私聊？旧私聊会归档：仍可在管家搜索中只读查看，助手检索历史时也能找到并知道它已归档。正在进行的回复和协作会停止，长期记忆不受影响。`
                : `确认删除「${confirmation?.assistant.name}」？助手配置将被删除，已有对话转移到快速使用助手，此操作不可恢复。`}
            onConfirm={() => { void confirm(); }} onCancel={() => { if (!busy.current) setConfirmation(null); }} />
    </>;
    return { run, dialogs };
}
