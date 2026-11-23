import { useEffect, useRef, useState } from "react";
import { AlertTriangle } from "lucide-react";
import FormDialog from "@/components/FormDialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import type { VaultItem } from "@/data/Vault";
import { cn } from "@/utils/utils";
import { keyError } from "./vaultHelpers";
import type { VaultItemPatch } from "./VaultItemDetail";

interface VaultItemEditDialogProps {
    item: VaultItem;
    open: boolean;
    takenKeys: string[];
    syncReady: boolean;
    onClose: () => void;
    onUpdate: (patch: VaultItemPatch) => Promise<boolean>;
}

const isEnterSubmit = (event: React.KeyboardEvent) =>
    event.key === "Enter" && !event.nativeEvent.isComposing && event.keyCode !== 229;

/**
 * 键输入框：前后缀把它和占位符的关系直接画出来，不必再写一段说明。
 * 输入框按内容定宽（field-sizing: content；不支持时退回等宽字体下的 size 字符数），让「>」紧跟在键后面；点空白处也能聚焦。
 */
export function VaultKeyInput({ id, value, invalid, describedBy, onChange, onKeyDown }: {
    id: string;
    value: string;
    invalid?: boolean;
    describedBy?: string;
    onChange: (value: string) => void;
    onKeyDown?: React.KeyboardEventHandler<HTMLInputElement>;
}) {
    const inputRef = useRef<HTMLInputElement>(null);
    return (
        <div
            className={cn(
                "flex min-w-0 cursor-text items-center overflow-x-auto rounded-md border bg-transparent font-mono text-sm shadow-xs transition-[color,box-shadow] dark:bg-input/30",
                invalid
                    ? "border-destructive focus-within:ring-[3px] focus-within:ring-destructive/20 dark:focus-within:ring-destructive/40"
                    : "border-input focus-within:border-ring focus-within:ring-[3px] focus-within:ring-ring/50",
            )}
            onMouseDown={(event) => {
                if (event.target === inputRef.current) return;
                event.preventDefault();
                inputRef.current?.focus();
            }}
        >
            <span className="shrink-0 select-none pl-3 text-muted-foreground" aria-hidden>{"<vault:"}</span>
            <Input
                ref={inputRef}
                id={id}
                size={value ? value.length : 12}
                className="h-9 w-auto min-w-0 flex-none rounded-none border-0 bg-transparent px-0 font-mono shadow-none [field-sizing:content] focus-visible:ring-0 aria-invalid:ring-0 dark:bg-transparent"
                autoCapitalize="none"
                spellCheck={false}
                placeholder="github_token"
                value={value}
                aria-invalid={invalid || undefined}
                aria-describedby={describedBy}
                onChange={(event) => onChange(event.target.value.trim())}
                onKeyDown={onKeyDown}
            />
            <span className="shrink-0 select-none pr-3 text-muted-foreground" aria-hidden>{">"}</span>
        </div>
    );
}

export default function VaultItemEditDialog({ item, open, takenKeys, syncReady, onClose, onUpdate }: VaultItemEditDialogProps) {
    const [name, setName] = useState(item.name);
    const [key, setKey] = useState(item.key);
    const [description, setDescription] = useState(item.description);
    const [syncEnabled, setSyncEnabled] = useState(item.syncEnabled);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        if (!open) return;
        setName(item.name);
        setKey(item.key);
        setDescription(item.description);
        setSyncEnabled(item.syncEnabled);
        setSaving(false);
        setError(null);
    }, [open, item.id, item.name, item.key, item.description, item.syncEnabled]);

    const keyChanged = key !== item.key;
    const keyProblem = keyChanged ? keyError(key, takenKeys.filter((k) => k !== item.key)) : null;

    const submit = async () => {
        if (saving) return;
        const trimmedName = name.trim();
        const problem = !trimmedName ? "请填写名称" : keyProblem;
        if (problem) {
            setError(problem);
            return;
        }
        const patch: VaultItemPatch = {};
        if (trimmedName !== item.name) patch.name = trimmedName;
        if (key !== item.key) patch.key = key;
        if (description.trim() !== item.description) patch.description = description.trim();
        if (syncEnabled !== item.syncEnabled) patch.syncEnabled = syncEnabled;
        if (Object.keys(patch).length === 0) {
            onClose();
            return;
        }
        setSaving(true);
        try {
            if (await onUpdate(patch)) onClose();
        } finally {
            setSaving(false);
        }
    };

    const handleEnterSubmit: React.KeyboardEventHandler<HTMLInputElement> = (event) => {
        if (!isEnterSubmit(event)) return;
        event.preventDefault();
        void submit();
    };

    return (
        <FormDialog
            title="编辑条目"
            subtitle={item.name}
            isOpen={open}
            submitting={saving}
            onClose={onClose}
            onSubmit={() => void submit()}
        >
            <div className="space-y-4">
                <div className="space-y-1.5">
                    <Label htmlFor="vault-edit-name">名称</Label>
                    <Input
                        id="vault-edit-name"
                        autoFocus
                        autoComplete="off"
                        value={name}
                        onChange={(event) => {
                            setName(event.target.value);
                            if (error) setError(null);
                        }}
                        onKeyDown={handleEnterSubmit}
                    />
                </div>
                <div className="space-y-1.5">
                    <Label htmlFor="vault-edit-key">键</Label>
                    <VaultKeyInput
                        id="vault-edit-key"
                        value={key}
                        invalid={!!keyProblem}
                        describedBy={keyChanged ? "vault-edit-key-hint" : undefined}
                        onChange={(next) => {
                            setKey(next);
                            if (error) setError(null);
                        }}
                        onKeyDown={handleEnterSubmit}
                    />
                    {keyChanged && (
                        <p id="vault-edit-key-hint" className={keyProblem ? "text-xs text-destructive" : "text-xs text-muted-foreground"}>
                            {keyProblem ?? "保存后助手要改用新的占位符；配置里的引用不受影响。"}
                        </p>
                    )}
                </div>
                <div className="space-y-1.5">
                    <Label htmlFor="vault-edit-description">给助手的说明</Label>
                    <Textarea
                        id="vault-edit-description"
                        rows={2}
                        className="resize-none"
                        placeholder="例如：用于读取我的 GitHub 仓库和 Issue"
                        value={description}
                        onChange={(event) => setDescription(event.target.value)}
                    />
                    <p className="text-xs text-muted-foreground">助手能看到这段说明，不要在这里填写值本身。</p>
                </div>
                <div className="flex items-center justify-between gap-4 rounded-lg border border-border px-3 py-2.5">
                    <div className="flex min-w-0 flex-col gap-0.5">
                        <Label htmlFor="vault-edit-sync" className="text-sm font-medium text-foreground">同步到我的其他设备</Label>
                        <p className="text-xs text-muted-foreground">
                            {syncReady
                                ? "端到端加密，服务器只能看到密文。关闭后会从其他设备移除。"
                                : "设置同步口令后才会同步，现在只保存在本机。"}
                        </p>
                    </div>
                    <Switch id="vault-edit-sync" checked={syncEnabled} onCheckedChange={setSyncEnabled} />
                </div>
                {error && (
                    <div role="alert" className="flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">
                        <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                        <span className="min-w-0 flex-1 break-words">{error}</span>
                    </div>
                )}
            </div>
        </FormDialog>
    );
}
