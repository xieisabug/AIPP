import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, KeyRound, Lock, Search, ShieldCheck } from "lucide-react";
import FormDialog from "@/components/FormDialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import type { VaultCreateInput, VaultItem } from "@/data/Vault";
import { cn } from "@/utils/utils";
import { suggestKey } from "./vaultHelpers";
import { containsVaultRef, suggestEnvName, type VaultRef } from "./vaultRefs";

export interface VaultPick {
    ref: VaultRef;
    /** 从当前值拆出的前缀（如 "Bearer "），选择已有条目时为空。 */
    prefix: string;
    variableName?: string;
    /** 当前值转存时，调用方必须保留空前缀，不能自动补 Bearer。 */
    storedCurrentValue?: boolean;
}

interface VaultPickerDialogProps {
    open: boolean;
    onClose: () => void;
    items: VaultItem[];
    /** 字段里现有的明文；有值时可以一键存入保险箱。 */
    currentValue?: string;
    /** 存入保险箱时预填的名称，例如「OpenAI · API Key」。 */
    suggestedName: string;
    /** 保存配置后会被确认的目标，例如 https://api.openai.com。 */
    targetHint?: string | null;
    /** 环境变量模式：额外询问变量名。 */
    askVariableName?: boolean;
    onPick: (pick: VaultPick) => void;
}

type Mode = "existing" | "store";

const isEnterSubmit = (event: React.KeyboardEvent) =>
    event.key === "Enter" && !event.nativeEvent.isComposing && event.keyCode !== 229;

/** 从保险箱选择条目，或把当前填写的明文存入保险箱并换成引用。 */
export default function VaultPickerDialog({
    open, onClose, items, currentValue, suggestedName, targetHint, askVariableName, onPick,
}: VaultPickerDialogProps) {
    // trim 只用于判断空值；转存必须保留原值的空格、制表符和换行。
    const plain = currentValue?.trim() && !containsVaultRef(currentValue) ? currentValue : "";
    const [mode, setMode] = useState<Mode>("existing");
    const [query, setQuery] = useState("");
    const [selectedId, setSelectedId] = useState<string>("");
    const [name, setName] = useState(suggestedName);
    const [variableName, setVariableName] = useState("");
    const [error, setError] = useState<string | null>(null);
    const [saving, setSaving] = useState(false);

    useEffect(() => {
        if (!open) return;
        setMode(items.length === 0 && plain ? "store" : "existing");
        setQuery("");
        setSelectedId(items[0]?.id ?? "");
        setName(suggestedName);
        setVariableName("");
        setError(null);
        // 只在打开时初始化，避免列表刷新时覆盖用户的选择。
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [open]);

    const filtered = useMemo(() => {
        const q = query.trim().toLowerCase();
        return q ? items.filter((i) => `${i.name} ${i.key} ${i.description}`.toLowerCase().includes(q)) : items;
    }, [items, query]);
    const selected = items.find((i) => i.id === selectedId);

    const effectiveVariable = variableName.trim() || (mode === "existing" && selected ? suggestEnvName(selected.key) : suggestEnvName(suggestKey(name, [])));

    const submit = async () => {
        if (saving) return;
        setError(null);
        if (askVariableName && !/^[A-Za-z_][A-Za-z0-9_]*$/.test(effectiveVariable)) {
            setError("变量名只能包含字母、数字和下划线，且不能以数字开头");
            return;
        }
        if (mode === "existing") {
            if (!selected) {
                setError("请选择一个条目");
                return;
            }
            onPick({ ref: { id: selected.id }, prefix: "", variableName: askVariableName ? effectiveVariable : undefined });
            onClose();
            return;
        }
        if (!name.trim()) {
            setError("请填写名称");
            return;
        }
        // 「Bearer xxx」只把 xxx 存进保险箱，前缀留在配置里。
        const prefix = /^bearer\s+/i.exec(plain)?.[0] ?? "";
        const input: VaultCreateInput = {
            name: name.trim(),
            key: suggestKey(name, items.map((i) => i.key)),
            description: "",
            value: plain.slice(prefix.length),
            allowedSites: [],
            syncEnabled: true,
        };
        setSaving(true);
        try {
            const created = await invoke<VaultItem>("vault_create", { input });
            onPick({ ref: { id: created.id }, prefix, variableName: askVariableName ? effectiveVariable : undefined, storedCurrentValue: true });
            onClose();
        } catch (e) {
            setError(String(e));
        } finally {
            setSaving(false);
        }
    };

    const handleEnterSubmit: React.KeyboardEventHandler<HTMLInputElement> = (event) => {
        if (!isEnterSubmit(event)) return;
        event.preventDefault();
        void submit();
    };

    const subtitle = mode === "store"
        ? "加密保存当前值"
        : items.length > 0
            ? `${items.length} 个可用条目`
            : undefined;

    return (
        <FormDialog
            title="使用凭据保险箱"
            subtitle={subtitle}
            isOpen={open}
            submitting={saving}
            onClose={onClose}
            onSubmit={() => void submit()}
        >
            <div className="space-y-4" data-aipp-slot="vault-picker">
                {plain && (
                    <RadioGroup
                        value={mode}
                        onValueChange={(v) => {
                            setMode(v as Mode);
                            setError(null);
                        }}
                        className="grid grid-cols-1 gap-2 sm:grid-cols-2"
                    >
                        <label
                            htmlFor="vault-pick-existing"
                            className={cn(
                                "flex cursor-pointer items-center gap-2.5 rounded-lg border border-border px-3 py-2.5 transition-colors hover:border-foreground/30 hover:bg-muted/60",
                                mode === "existing" && "border-foreground/50 bg-muted/60",
                            )}
                        >
                            <RadioGroupItem id="vault-pick-existing" value="existing" />
                            <span className="min-w-0 flex-1 text-sm font-medium">选择已有条目</span>
                        </label>
                        <label
                            htmlFor="vault-pick-store"
                            className={cn(
                                "flex cursor-pointer items-center gap-2.5 rounded-lg border border-border px-3 py-2.5 transition-colors hover:border-foreground/30 hover:bg-muted/60",
                                mode === "store" && "border-foreground/50 bg-muted/60",
                            )}
                        >
                            <RadioGroupItem id="vault-pick-store" value="store" />
                            <span className="min-w-0 flex-1 text-sm font-medium">把当前填写的值存入保险箱</span>
                        </label>
                    </RadioGroup>
                )}

                {mode === "existing" ? (
                    items.length === 0 ? (
                        <div className="flex flex-col items-center gap-2 rounded-lg border border-dashed border-border bg-muted/20 px-4 py-6 text-center">
                            <KeyRound className="h-5 w-5 text-muted-foreground" />
                            <p className="text-sm text-muted-foreground">保险箱里还没有条目。可以先在「设置 → 凭据保险箱」中新建。</p>
                        </div>
                    ) : (
                        <div className="space-y-2.5">
                            <div className="relative">
                                <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
                                <Input
                                    placeholder="搜索名称或键..."
                                    value={query}
                                    onChange={(e) => setQuery(e.target.value)}
                                    aria-label="搜索条目"
                                    className="h-8 pl-8 text-sm"
                                />
                            </div>
                            <RadioGroup value={selectedId} onValueChange={setSelectedId} className="max-h-60 gap-1.5 overflow-y-auto pr-0.5">
                                {filtered.map((item) => (
                                    <label
                                        key={item.id}
                                        htmlFor={`vault-pick-${item.id}`}
                                        className={cn(
                                            "flex cursor-pointer items-center gap-2.5 rounded-lg border border-border px-3 py-2 transition-colors hover:bg-muted/60",
                                            selectedId === item.id && "border-foreground/40 bg-muted/60",
                                        )}
                                    >
                                        <RadioGroupItem id={`vault-pick-${item.id}`} value={item.id} aria-label={item.name} />
                                        <span className="min-w-0 flex-1">
                                            <span className="flex items-center gap-2">
                                                <span className="truncate text-sm font-medium">{item.name}</span>
                                                {item.status === "locked" && (
                                                    <span className="inline-flex shrink-0 items-center rounded border border-border bg-background px-1.5 py-px text-[11px] leading-4 text-muted-foreground">
                                                        待解锁
                                                    </span>
                                                )}
                                            </span>
                                            <span className="block truncate font-mono text-xs text-muted-foreground">
                                                {item.key}
                                            </span>
                                        </span>
                                    </label>
                                ))}
                                {filtered.length === 0 && (
                                    <p className="py-6 text-center text-sm text-muted-foreground">没有匹配的条目</p>
                                )}
                            </RadioGroup>
                        </div>
                    )
                ) : (
                    <div className="space-y-3">
                        <div className="space-y-1.5">
                            <Label htmlFor="vault-pick-name">名称</Label>
                            <Input
                                id="vault-pick-name"
                                autoFocus
                                autoComplete="off"
                                value={name}
                                onChange={(e) => {
                                    setName(e.target.value);
                                    if (error) setError(null);
                                }}
                                onKeyDown={handleEnterSubmit}
                            />
                        </div>
                        <div className="flex items-start gap-2 rounded-lg bg-muted/50 px-3 py-2 text-xs text-muted-foreground">
                            <Lock className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                            <span>当前填写的值会加密存入保险箱，这里改为引用它；之后不会再显示这个值。</span>
                        </div>
                    </div>
                )}

                {askVariableName && (
                    <div className="space-y-1.5">
                        <Label htmlFor="vault-pick-variable">变量名</Label>
                        <Input
                            id="vault-pick-variable"
                            autoComplete="off"
                            value={variableName}
                            placeholder={effectiveVariable}
                            onChange={(e) => {
                                setVariableName(e.target.value);
                                if (error) setError(null);
                            }}
                            onKeyDown={handleEnterSubmit}
                            className="font-mono"
                        />
                    </div>
                )}

                {targetHint && (
                    <div className="flex items-start gap-2 rounded-lg bg-muted/50 px-3 py-2 text-xs text-muted-foreground">
                        <ShieldCheck className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                        <span className="min-w-0 break-all">保存配置后，这个值将被确认用于 {targetHint}</span>
                    </div>
                )}

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
