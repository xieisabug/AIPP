import { useEffect, useState, type ReactNode } from "react";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    CompactDialogFooter,
} from "@/components/ui/dialog";
import type { AssistantListItem } from "@/data/Assistant";
import { cn } from "@/utils/utils";
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
        <label
            key={key}
            className={cn(
                "flex cursor-pointer items-center gap-3 rounded-lg border border-transparent px-2.5 py-2 transition-colors hover:bg-muted/60",
                checked && "border-foreground/30 bg-muted/60",
            )}
        >
            <Checkbox checked={checked} onCheckedChange={onToggle} disabled={busy} aria-label={label} />
            {avatar}
            <span className="min-w-0 flex-1 truncate text-sm">{label}</span>
        </label>;
    return <Dialog open={open} onOpenChange={value => { if (!busy) onOpenChange(value); }}>
        <CompactDialogContent className="sm:max-w-md">
            <CompactDialogHeader
                title={mode === "create" ? "发起群聊" : "邀请成员"}
                subtitle={`已选 ${count} 位`}
                description={mode === "create" ? "选择要拉进群的助手。群里直接发消息就行，会由合适的成员回复。" : "新成员只会看到加入之后的群消息。"}
                closeDisabled={busy}
            />
            <CompactDialogBody className="space-y-3">
                <p className="text-xs text-muted-foreground">
                    {mode === "create" ? "选择要拉进群的助手。群里直接发消息就行，会由合适的成员回复。" : "新成员只会看到加入之后的群消息。"}
                </p>
                {mode === "create" && <Input aria-label="群聊名称" placeholder="群聊名称（可不填）" value={name} maxLength={60}
                    disabled={busy} onChange={event => setName(event.target.value)} />}
                <div className="max-h-72 space-y-1 overflow-y-auto pr-0.5" role="group" aria-label="选择成员">
                    {showChief && row("chief", includeChief, () => setIncludeChief(value => !value),
                        <ButlerAvatar id={0} chief size={28} figure />, chiefName)}
                    {candidates.map(assistant => row(String(assistant.id), selected.includes(assistant.id), () => toggle(assistant.id),
                        <ButlerAvatar id={assistant.id} size={28} figure />, assistant.name))}
                    {!showChief && !candidates.length && <p className="px-2 py-6 text-center text-sm text-muted-foreground">所有助手都已经在群里了</p>}
                </div>
                {error && <p role="alert" className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 whitespace-pre-wrap text-xs text-destructive">{error}</p>}
            </CompactDialogBody>
            <CompactDialogFooter>
                <span className="mr-auto text-xs text-muted-foreground">已选 {count} 位</span>
                <Button type="button" variant="outline" className="min-h-11 px-4 sm:min-h-9" disabled={busy} onClick={() => onOpenChange(false)}>
                    取消
                </Button>
                <Button type="button" className="min-h-11 px-5 sm:min-h-9" disabled={busy || count < minimum} onClick={() => void confirm()}>
                    {busy && <Loader2 className="h-4 w-4 animate-spin" />}
                    {busy ? "请稍候…" : mode === "create" ? "创建群聊" : "邀请"}
                </Button>
            </CompactDialogFooter>
        </CompactDialogContent>
    </Dialog>;
}
