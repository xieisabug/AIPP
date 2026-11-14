import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import FormDialog from "@/components/FormDialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { VAULT_AUTH_KIND_LABELS, type VaultItem } from "@/data/Vault";
import { suggestHandle } from "./vaultHelpers";
import { containsVaultRef, suggestEnvName, type VaultRef } from "./vaultRefs";

export interface VaultPick {
    ref: VaultRef;
    /** 从当前值拆出的前缀（如 "Bearer "），已有凭据时为空。 */
    prefix: string;
    variableName?: string;
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

/** 从保险箱选择凭据，或把当前填写的明文存入保险箱并换成引用。 */
export default function VaultPickerDialog({
    open, onClose, items, currentValue, suggestedName, targetHint, askVariableName, onPick,
}: VaultPickerDialogProps) {
    const plain = currentValue?.trim() && !containsVaultRef(currentValue) ? currentValue.trim() : "";
    const [mode, setMode] = useState<Mode>("existing");
    const [query, setQuery] = useState("");
    const [selectedId, setSelectedId] = useState<string>("");
    const [field, setField] = useState("");
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
        return q ? items.filter((i) => `${i.name} ${i.handle}`.toLowerCase().includes(q)) : items;
    }, [items, query]);
    const selected = items.find((i) => i.id === selectedId);

    useEffect(() => {
        setField(selected?.fields[0]?.key ?? "");
    }, [selected]);

    const effectiveVariable = variableName.trim() || (mode === "existing" && selected ? suggestEnvName(selected.handle) : suggestEnvName(suggestHandle(name, [])));

    const submit = async () => {
        setError(null);
        if (askVariableName && !/^[A-Za-z_][A-Za-z0-9_]*$/.test(effectiveVariable)) {
            setError("变量名只能包含字母、数字和下划线，且不能以数字开头");
            return;
        }
        if (mode === "existing") {
            if (!selected || !field) {
                setError("请选择一条凭据");
                return;
            }
            onPick({ ref: { id: selected.id, field }, prefix: "", variableName: askVariableName ? effectiveVariable : undefined });
            onClose();
            return;
        }
        if (!name.trim()) {
            setError("请填写名称");
            return;
        }
        const prefix = /^bearer\s+/i.exec(plain)?.[0] ?? "";
        setSaving(true);
        try {
            const created = await invoke<VaultItem>("vault_create", {
                input: {
                    name: name.trim(),
                    handle: suggestHandle(name, items.map((i) => i.handle)),
                    description: "",
                    auth: { kind: "bearer", token: plain.slice(prefix.length) },
                    allowedSites: [],
                    syncEnabled: true,
                },
            });
            onPick({ ref: { id: created.id, field: "token" }, prefix, variableName: askVariableName ? effectiveVariable : undefined });
            onClose();
        } catch (e) {
            setError(String(e));
        } finally {
            setSaving(false);
        }
    };

    return (
        <FormDialog title="使用凭据保险箱" isOpen={open} onClose={onClose} onSubmit={() => { if (!saving) void submit(); }}>
            <div className="space-y-4" data-aipp-slot="vault-picker">
                {plain && (
                    <RadioGroup value={mode} onValueChange={(v) => setMode(v as Mode)} className="flex gap-4">
                        <div className="flex items-center gap-2">
                            <RadioGroupItem id="vault-pick-existing" value="existing" />
                            <Label htmlFor="vault-pick-existing">选择已有凭据</Label>
                        </div>
                        <div className="flex items-center gap-2">
                            <RadioGroupItem id="vault-pick-store" value="store" />
                            <Label htmlFor="vault-pick-store">把当前填写的值存入保险箱</Label>
                        </div>
                    </RadioGroup>
                )}

                {mode === "existing" ? (
                    items.length === 0 ? (
                        <p className="text-sm text-muted-foreground">保险箱里还没有凭据。可以先在「设置 → 凭据保险箱」中新建。</p>
                    ) : (
                        <div className="space-y-3">
                            <Input placeholder="搜索名称或引用名" value={query} onChange={(e) => setQuery(e.target.value)} aria-label="搜索凭据" />
                            <RadioGroup value={selectedId} onValueChange={setSelectedId} className="max-h-56 gap-1 overflow-y-auto">
                                {filtered.map((item) => (
                                    <label
                                        key={item.id}
                                        htmlFor={`vault-pick-${item.id}`}
                                        className="flex cursor-pointer items-center gap-3 rounded-md border border-border px-3 py-2 hover:bg-muted/50"
                                    >
                                        <RadioGroupItem id={`vault-pick-${item.id}`} value={item.id} aria-label={item.name} />
                                        <span className="min-w-0 flex-1">
                                            <span className="block truncate text-sm font-medium">{item.name}</span>
                                            <span className="block truncate text-xs text-muted-foreground">
                                                {item.handle} · {VAULT_AUTH_KIND_LABELS[item.authKind]}
                                                {item.status === "locked" ? " · 待解锁" : ""}
                                            </span>
                                        </span>
                                    </label>
                                ))}
                                {filtered.length === 0 && <p className="px-1 text-sm text-muted-foreground">没有匹配的凭据</p>}
                            </RadioGroup>
                            {selected && selected.fields.length > 1 && (
                                <div className="space-y-1">
                                    <Label>使用的值</Label>
                                    <Select value={field} onValueChange={setField}>
                                        <SelectTrigger aria-label="使用的值"><SelectValue /></SelectTrigger>
                                        <SelectContent>
                                            {selected.fields.map((f) => <SelectItem key={f.key} value={f.key}>{f.label}</SelectItem>)}
                                        </SelectContent>
                                    </Select>
                                </div>
                            )}
                        </div>
                    )
                ) : (
                    <div className="space-y-1">
                        <Label htmlFor="vault-pick-name">名称</Label>
                        <Input id="vault-pick-name" value={name} onChange={(e) => setName(e.target.value)} />
                        <p className="text-xs text-muted-foreground">当前填写的值会加密存入保险箱，这里改为引用它；之后不会再显示这个值。</p>
                    </div>
                )}

                {askVariableName && (
                    <div className="space-y-1">
                        <Label htmlFor="vault-pick-variable">变量名</Label>
                        <Input id="vault-pick-variable" value={variableName} placeholder={effectiveVariable} onChange={(e) => setVariableName(e.target.value)} className="font-mono" />
                    </div>
                )}

                {targetHint && <p className="text-xs text-muted-foreground">保存配置后，此凭据将被确认用于 {targetHint}</p>}
                {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
            </div>
        </FormDialog>
    );
}
