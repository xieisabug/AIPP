import { useEffect, useState, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import type { AssistantListItem } from "@/data/Assistant";
import ButlerAvatar from "./ButlerAvatar";

export interface GroupMemberPickerResult {
    name: string;
    assistantIds: number[];
    includeChief: boolean;
}

interface Props {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    mode: "create" | "invite";
    chiefName: string;
    assistants: AssistantListItem[];
    /** 已在群里的助手不再出现在可选列表中。 */
    existingAssistantIds?: number[];
    chiefInGroup?: boolean;
    onConfirm: (result: GroupMemberPickerResult) => Promise<void>;
}

/** 发起群聊 / 邀请成员：像聊天软件一样勾选联系人即可。 */
export default function ButlerGroupMemberPicker({ open, onOpenChange, mode, chiefName, assistants, existingAssistantIds = [], chiefInGroup = false, onConfirm }: Props) {
    const [name, setName] = useState("");
    const [selected, setSelected] = useState<number[]>([]);
    const [includeChief, setIncludeChief] = useState(false);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState("");
    useEffect(() => {
        if (!open) return;
        setName("");
        setSelected([]);
        setIncludeChief(mode === "create");
        setError("");
    }, [open, mode]);
    const candidates = assistants.filter(assistant => !existingAssistantIds.includes(assistant.id));
    const showChief = !chiefInGroup;
    const count = selected.length + (includeChief && showChief ? 1 : 0);
    const minimum = mode === "create" ? 2 : 1;
    const toggle = (id: number) => setSelected(previous => previous.includes(id) ? previous.filter(value => value !== id) : [...previous, id]);
    const confirm = async () => {
        setBusy(true);
        setError("");
        try {
            await onConfirm({ name, assistantIds: selected, includeChief: includeChief && showChief });
            onOpenChange(false);
        } catch (reason) {
            setError(String(reason));
        } finally {
            setBusy(false);
        }
    };
    const row = (key: string, checked: boolean, onToggle: () => void, avatar: ReactNode, label: string) =>
        <label key={key} className="flex cursor-pointer items-center gap-3 rounded-lg px-2 py-1.5 hover:bg-muted">
            <Checkbox checked={checked} onCheckedChange={onToggle} disabled={busy} aria-label={label} />
            {avatar}
            <span className="min-w-0 flex-1 truncate text-sm">{label}</span>
        </label>;
    return <Dialog open={open} onOpenChange={value => { if (!busy) onOpenChange(value); }}>
        <DialogContent className="sm:max-w-md">
            <DialogHeader>
                <DialogTitle>{mode === "create" ? "发起群聊" : "邀请成员"}</DialogTitle>
                <DialogDescription>
                    {mode === "create" ? "选择要拉进群的助手。群里直接发消息就行，会由合适的成员回复。" : "新成员只会看到加入之后的群消息。"}
                </DialogDescription>
            </DialogHeader>
            {mode === "create" && <Input aria-label="群聊名称" placeholder="群聊名称（可不填）" value={name} maxLength={60}
                disabled={busy} onChange={event => setName(event.target.value)} />}
            <div className="max-h-72 space-y-0.5 overflow-y-auto" role="group" aria-label="选择成员">
                {showChief && row("chief", includeChief, () => setIncludeChief(value => !value),
                    <ButlerAvatar id={0} chief size={28} figure />, chiefName)}
                {candidates.map(assistant => row(String(assistant.id), selected.includes(assistant.id), () => toggle(assistant.id),
                    <ButlerAvatar id={assistant.id} size={28} figure />, assistant.name))}
                {!showChief && !candidates.length && <p className="px-2 py-6 text-center text-sm text-muted-foreground">所有助手都已经在群里了</p>}
            </div>
            {error && <p role="alert" className="whitespace-pre-wrap text-sm text-destructive">{error}</p>}
            <DialogFooter className="items-center sm:justify-between">
                <span className="text-xs text-muted-foreground">已选 {count} 位</span>
                <Button disabled={busy || count < minimum} onClick={() => void confirm()}>
                    {busy ? "请稍候…" : mode === "create" ? "创建群聊" : "邀请"}
                </Button>
            </DialogFooter>
        </DialogContent>
    </Dialog>;
}
