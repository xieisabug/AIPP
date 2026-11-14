import React, { useState } from "react";
import { KeyRound } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { PasswordInput } from "@/components/ui/password-input";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import type { VaultItem } from "@/data/Vault";
import VaultPickerDialog, { type VaultPick } from "./VaultPickerDialog";
import { formatVaultRef, parseVaultValue } from "./vaultRefs";

export interface VaultFieldSource {
    items: VaultItem[];
    available: boolean;
}

interface VaultRefFieldProps {
    vault: VaultFieldSource;
    value: string;
    onChange: (value: string) => void;
    id?: string;
    ariaLabel?: string;
    placeholder?: string;
    disabled?: boolean;
    /** 允许在引用前写文字前缀（请求头里的 "Bearer "）。 */
    allowPrefix?: boolean;
    /** 请求头名：Authorization 配 Bearer 凭据时自动填 "Bearer "。 */
    headerName?: string;
    /** 把当前值存入保险箱时预填的名称。 */
    suggestedName: string;
    targetHint?: string | null;
}

/** 秘密值输入框：可以直接填写，也可以引用保险箱里的凭据（显示为只读标签，不显示值）。 */
export default function VaultRefField({
    vault, value, onChange, id, ariaLabel, placeholder, disabled, allowPrefix, headerName, suggestedName, targetHint,
}: VaultRefFieldProps) {
    const isMobile = useIsMobilePlatform();
    const [picking, setPicking] = useState(false);
    const parsed = parseVaultValue(value);
    const item = parsed ? vault.items.find((i) => i.id === parsed.id) : undefined;

    const pick = ({ ref, prefix }: VaultPick) => {
        const chosen = vault.items.find((i) => i.id === ref.id);
        const bearer = !prefix && allowPrefix && headerName?.toLowerCase() === "authorization" && (chosen?.authKind ?? "bearer") === "bearer";
        onChange(formatVaultRef(ref, prefix || (bearer ? "Bearer " : parsed?.prefix ?? "")));
    };

    const picker = (
        <VaultPickerDialog
            open={picking}
            onClose={() => setPicking(false)}
            items={vault.items}
            currentValue={parsed ? undefined : value}
            suggestedName={suggestedName}
            targetHint={targetHint}
            onPick={pick}
        />
    );

    if (parsed) {
        const fieldLabel = item?.fields.find((f) => f.key === parsed.field)?.label;
        const label = item
            ? `${item.name}${fieldLabel ? ` · ${fieldLabel}` : ""}`
            : vault.available ? "凭据已删除，请重新选择" : "此设备不支持凭据保险箱";
        return (
            <div className="flex min-w-0 items-center gap-2" data-aipp-slot="vault-ref-field">
                {allowPrefix && (
                    <Input
                        aria-label="前缀"
                        value={parsed.prefix}
                        placeholder="前缀"
                        disabled={disabled}
                        onChange={(e) => onChange(formatVaultRef(parsed, e.target.value))}
                        className="w-24 shrink-0 font-mono"
                    />
                )}
                <div
                    id={id}
                    aria-label={ariaLabel}
                    className="flex min-h-9 min-w-0 flex-1 items-center gap-2 rounded-md border border-border bg-muted/40 px-3 text-sm"
                >
                    <KeyRound className="h-4 w-4 shrink-0 text-muted-foreground" />
                    <span className={`min-w-0 flex-1 truncate ${item ? "" : "text-muted-foreground line-through"}`} title="值保存在凭据保险箱中，只会发往已确认的网站">
                        {label}
                    </span>
                    {vault.available && (
                        <Button type="button" variant="ghost" size="sm" className="h-7 px-2" disabled={disabled} onClick={() => setPicking(true)}>
                            更换
                        </Button>
                    )}
                    <Button type="button" variant="ghost" size="sm" className="h-7 px-2" disabled={disabled} onClick={() => onChange("")}>
                        移除
                    </Button>
                </div>
                {picker}
            </div>
        );
    }

    const inputProps = {
        id, value, placeholder, disabled, autoComplete: "off", "aria-label": ariaLabel, className: "min-w-0 flex-1",
        onChange: (e: React.ChangeEvent<HTMLInputElement>) => onChange(e.target.value),
    };
    return (
        <div className="flex min-w-0 items-center gap-2" data-aipp-slot="vault-ref-field">
            {isMobile ? <PasswordInput {...inputProps} /> : <Input type="password" {...inputProps} />}
            {vault.available && (
                <Button
                    type="button"
                    variant="outline"
                    size="icon"
                    className="shrink-0"
                    aria-label="从凭据保险箱选择"
                    title="从凭据保险箱选择"
                    disabled={disabled}
                    onClick={() => setPicking(true)}
                >
                    <KeyRound className="h-4 w-4" />
                </Button>
            )}
            {picker}
        </div>
    );
}
