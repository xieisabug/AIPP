import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { PasswordInput } from "@/components/ui/password-input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { VAULT_AUTH_KIND_LABELS, type VaultAuthInput, type VaultAuthKind, type VaultNamedSecret } from "@/data/Vault";
import { emptyAuth } from "./vaultHelpers";

interface VaultAuthFieldsProps {
    value: VaultAuthInput;
    onChange: (value: VaultAuthInput) => void;
    idPrefix: string;
}

const KIND_HINTS: Record<VaultAuthKind, string> = {
    bearer: "以 Authorization: Bearer <令牌> 发送，适用于大多数 API。",
    header: "以你指定的请求头发送，例如 X-Api-Key。可以添加多个。",
    basic: "以 Authorization: Basic 发送用户名和密码。",
    query: "作为 URL 参数发送（不推荐：参数可能出现在服务端日志中）。",
};

const KINDS: VaultAuthKind[] = ["bearer", "header", "basic", "query"];

/** 认证方式选择 + 值输入。值只在提交时发给后端，之后不会再回显。 */
export default function VaultAuthFields({ value, onChange, idPrefix }: VaultAuthFieldsProps) {
    const rows = (value.kind === "header" ? value.headers : value.query) ?? [];
    const setRows = (next: VaultNamedSecret[]) =>
        onChange(value.kind === "header" ? { ...value, headers: next } : { ...value, query: next });
    const updateRow = (index: number, patch: Partial<VaultNamedSecret>) =>
        setRows(rows.map((row, i) => (i === index ? { ...row, ...patch } : row)));

    return (
        <div className="space-y-4">
            <div className="space-y-2">
                <Label htmlFor={`${idPrefix}-kind`}>认证方式</Label>
                <Select value={value.kind} onValueChange={(kind) => onChange(emptyAuth(kind as VaultAuthKind))}>
                    <SelectTrigger id={`${idPrefix}-kind`} className="w-full">
                        <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                        {KINDS.map((kind) => (
                            <SelectItem key={kind} value={kind}>{VAULT_AUTH_KIND_LABELS[kind]}</SelectItem>
                        ))}
                    </SelectContent>
                </Select>
                <p className="text-xs text-muted-foreground">{KIND_HINTS[value.kind]}</p>
            </div>

            {value.kind === "bearer" && (
                <div className="space-y-2">
                    <Label htmlFor={`${idPrefix}-token`}>令牌</Label>
                    <PasswordInput
                        id={`${idPrefix}-token`}
                        autoComplete="off"
                        spellCheck={false}
                        placeholder="粘贴令牌或 API Key"
                        value={value.token ?? ""}
                        onChange={(event) => onChange({ ...value, token: event.target.value })}
                    />
                </div>
            )}

            {value.kind === "basic" && (
                <>
                    <div className="space-y-2">
                        <Label htmlFor={`${idPrefix}-username`}>用户名</Label>
                        <Input
                            id={`${idPrefix}-username`}
                            autoComplete="off"
                            spellCheck={false}
                            value={value.username ?? ""}
                            onChange={(event) => onChange({ ...value, username: event.target.value })}
                        />
                    </div>
                    <div className="space-y-2">
                        <Label htmlFor={`${idPrefix}-password`}>密码</Label>
                        <PasswordInput
                            id={`${idPrefix}-password`}
                            autoComplete="off"
                            value={value.password ?? ""}
                            onChange={(event) => onChange({ ...value, password: event.target.value })}
                        />
                    </div>
                </>
            )}

            {(value.kind === "header" || value.kind === "query") && (
                <div className="space-y-2">
                    <Label>{value.kind === "header" ? "请求头" : "URL 参数"}</Label>
                    {rows.map((row, index) => (
                        <div key={index} className="flex items-start gap-2">
                            <Input
                                aria-label={value.kind === "header" ? "请求头名称" : "参数名"}
                                className="w-2/5"
                                autoComplete="off"
                                spellCheck={false}
                                placeholder={value.kind === "header" ? "X-Api-Key" : "key"}
                                value={row.name}
                                onChange={(event) => updateRow(index, { name: event.target.value })}
                            />
                            <div className="flex-1">
                                <PasswordInput
                                    aria-label={value.kind === "header" ? "请求头的值" : "参数值"}
                                    autoComplete="off"
                                    placeholder="值"
                                    value={row.value}
                                    onChange={(event) => updateRow(index, { value: event.target.value })}
                                />
                            </div>
                            {rows.length > 1 && (
                                <Button type="button" variant="ghost" size="icon" aria-label="移除这一行" onClick={() => setRows(rows.filter((_, i) => i !== index))}>
                                    <Trash2 className="h-4 w-4" />
                                </Button>
                            )}
                        </div>
                    ))}
                    {value.kind === "header" && (
                        <Button type="button" variant="outline" size="sm" className="gap-1" onClick={() => setRows([...rows, { name: "", prefix: "", value: "" }])}>
                            <Plus className="h-4 w-4" />
                            添加请求头
                        </Button>
                    )}
                </div>
            )}
        </div>
    );
}
