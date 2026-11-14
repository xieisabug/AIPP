import { useEffect, useState } from "react";
import { AlertTriangle, Check, Globe, KeyRound, Trash2, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { VAULT_AUTH_KIND_LABELS, type VaultConfigReference, type VaultItem, type VaultUsageEntry } from "@/data/Vault";
import { formatTime, handleError } from "./vaultHelpers";

export interface VaultItemPatch {
    name?: string;
    handle?: string;
    description?: string;
    allowedSites?: string[];
    syncEnabled?: boolean;
}

interface VaultItemDetailProps {
    item: VaultItem;
    usage: VaultUsageEntry[];
    takenHandles: string[];
    syncReady: boolean;
    /** 引用此凭据的 MCP / Provider 配置及其当前目标。 */
    references: VaultConfigReference[];
    onConfirmReference: (reference: VaultConfigReference) => void;
    onUpdate: (patch: VaultItemPatch) => Promise<boolean>;
    onReplace: () => void;
    onDelete: () => void;
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
    return (
        <section className="space-y-3">
            <h3 className="text-sm font-medium text-muted-foreground">{title}</h3>
            {children}
        </section>
    );
}

function outcomeText(entry: VaultUsageEntry): string {
    if (entry.outcome === "ok") return entry.status ? String(entry.status) : "完成";
    if (entry.outcome === "denied") return "已拒绝";
    return "失败";
}

export default function VaultItemDetail({ item, usage, takenHandles, syncReady, references, onConfirmReference, onUpdate, onReplace, onDelete }: VaultItemDetailProps) {
    const [name, setName] = useState(item.name);
    const [handle, setHandle] = useState(item.handle);
    const [description, setDescription] = useState(item.description);
    const [newSite, setNewSite] = useState("");
    const [saving, setSaving] = useState(false);

    useEffect(() => {
        setName(item.name);
        setHandle(item.handle);
        setDescription(item.description);
        setNewSite("");
    }, [item.id, item.name, item.handle, item.description]);

    const handleProblem = handle === item.handle ? null : handleError(handle, takenHandles.filter((h) => h !== item.handle));
    const dirty = name.trim() !== item.name || handle !== item.handle || description.trim() !== item.description;
    const canSave = dirty && !!name.trim() && !handleProblem && !saving;

    const save = async () => {
        setSaving(true);
        await onUpdate({ name: name.trim(), handle, description: description.trim() });
        setSaving(false);
    };

    const addSite = async () => {
        const site = newSite.trim();
        if (!site) return;
        if (await onUpdate({ allowedSites: [...item.allowedSites, site] })) setNewSite("");
    };

    return (
        <div className="space-y-8">
            <header className="flex items-start justify-between gap-4">
                <div className="min-w-0 space-y-1">
                    <div className="flex items-center gap-2">
                        <KeyRound className="h-5 w-5 shrink-0 text-muted-foreground" />
                        <h2 className="truncate text-xl font-semibold">{item.name}</h2>
                        {item.status === "locked" && <Badge variant="outline">待解锁</Badge>}
                    </div>
                    <p className="text-sm text-muted-foreground">
                        <code className="font-mono">{item.handle}</code> · {VAULT_AUTH_KIND_LABELS[item.authKind]}
                    </p>
                </div>
                <Button variant="outline" size="sm" className="gap-1" onClick={onDelete}>
                    <Trash2 className="h-4 w-4" />
                    删除
                </Button>
            </header>

            <Section title="凭据">
                <div className="space-y-3 rounded-lg border border-border p-4">
                    <div className="flex items-center justify-between gap-4">
                        <div className="min-w-0">
                            <div className="font-mono tracking-widest text-muted-foreground" aria-hidden>●●●●●●●●</div>
                            <p className="text-xs text-muted-foreground">
                                {item.status === "locked" ? "来自其他设备，输入同步口令后可用" : `已加密保存 · ${formatTime(item.updatedAt)} 更新 · 不会再显示`}
                            </p>
                        </div>
                        <Button variant="outline" size="sm" onClick={onReplace}>替换</Button>
                    </div>
                    <div className="space-y-1 border-t border-border pt-3">
                        <p className="text-xs text-muted-foreground">发送时注入为</p>
                        {item.injection.map((line) => (
                            <code key={line} className="block font-mono text-sm">{line}</code>
                        ))}
                    </div>
                </div>
            </Section>

            <Section title="基本信息">
                <div className="space-y-4">
                    <div className="space-y-2">
                        <Label htmlFor="vault-detail-name">名称</Label>
                        <Input id="vault-detail-name" value={name} onChange={(event) => setName(event.target.value)} />
                    </div>
                    <div className="space-y-2">
                        <Label htmlFor="vault-detail-handle">引用名</Label>
                        <Input
                            id="vault-detail-handle"
                            className="font-mono"
                            autoCapitalize="none"
                            spellCheck={false}
                            value={handle}
                            aria-invalid={handleProblem ? true : undefined}
                            onChange={(event) => setHandle(event.target.value.trim())}
                        />
                        <p className={handleProblem ? "text-xs text-destructive" : "text-xs text-muted-foreground"}>
                            {handleProblem ?? "助手用这个名字引用凭据。改名后，助手需要用新名字。"}
                        </p>
                    </div>
                    <div className="space-y-2">
                        <Label htmlFor="vault-detail-description">给助手的说明</Label>
                        <Textarea
                            id="vault-detail-description"
                            rows={3}
                            placeholder="例如：用于读取我的 GitHub 仓库和 Issue"
                            value={description}
                            onChange={(event) => setDescription(event.target.value)}
                        />
                        <p className="text-xs text-muted-foreground">助手可以看到这段说明，不要在这里填写密钥。</p>
                    </div>
                    <div className="flex justify-end">
                        <Button size="sm" disabled={!canSave} onClick={save}>保存</Button>
                    </div>
                </div>
            </Section>

            <Section title="允许的网站">
                <p className="text-xs text-muted-foreground">
                    助手第一次用这条凭据访问某个网站时会先问你；选择「总是允许」的网站会出现在这里，之后直接使用。
                </p>
                {item.allowedSites.length === 0 ? (
                    <p className="rounded-lg border border-dashed border-border p-3 text-sm text-muted-foreground">还没有允许的网站</p>
                ) : (
                    <ul className="divide-y divide-border rounded-lg border border-border">
                        {item.allowedSites.map((site) => (
                            <li key={site} className="flex items-center justify-between gap-2 px-3 py-2">
                                <span className="flex min-w-0 items-center gap-2">
                                    <Globe className="h-4 w-4 shrink-0 text-muted-foreground" />
                                    <span className="truncate font-mono text-sm">{site}</span>
                                </span>
                                <Button
                                    variant="ghost"
                                    size="icon"
                                    aria-label={`移除 ${site}`}
                                    onClick={() => onUpdate({ allowedSites: item.allowedSites.filter((s) => s !== site) })}
                                >
                                    <X className="h-4 w-4" />
                                </Button>
                            </li>
                        ))}
                    </ul>
                )}
                <div className="flex gap-2">
                    <Input
                        aria-label="添加允许的网站"
                        placeholder="https://api.example.com"
                        autoCapitalize="none"
                        spellCheck={false}
                        value={newSite}
                        onChange={(event) => setNewSite(event.target.value)}
                        onKeyDown={(event) => {
                            if (event.key === "Enter" && !event.nativeEvent.isComposing) {
                                event.preventDefault();
                                void addSite();
                            }
                        }}
                    />
                    <Button variant="outline" disabled={!newSite.trim()} onClick={() => void addSite()}>添加</Button>
                </div>
            </Section>

            <Section title="同步">
                <div className="flex items-start justify-between gap-4 rounded-lg border border-border p-4">
                    <div className="space-y-1">
                        <Label htmlFor="vault-detail-sync">同步到我的其他设备</Label>
                        <p className="text-xs text-muted-foreground">
                            {syncReady
                                ? "端到端加密，同步服务器只能看到密文。关闭后，其他设备上的这条凭据会被移除。"
                                : "设置同步口令后才会同步；未设置时只保存在本机。"}
                        </p>
                    </div>
                    <Switch
                        id="vault-detail-sync"
                        checked={item.syncEnabled}
                        onCheckedChange={(checked) => void onUpdate({ syncEnabled: checked })}
                    />
                </div>
            </Section>

            <Section title="被以下配置使用">
                {references.length === 0 ? (
                    <p className="text-sm text-muted-foreground">
                        没有配置引用它。可以在 MCP 的请求头、环境变量或 Provider 的 API Key 旁点钥匙按钮，从保险箱选择。
                    </p>
                ) : (
                    <ul className="divide-y divide-border rounded-lg border border-border text-sm" data-aipp-slot="vault-references">
                        {references.map((reference) => (
                            <li key={`${reference.kind}-${reference.configId}`} className="flex items-center gap-3 px-3 py-2">
                                <span className="w-40 shrink-0 truncate" title={reference.name}>{reference.name}</span>
                                <span className="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground" title={reference.target ?? reference.problem ?? undefined}>
                                    {reference.target ?? reference.problem}
                                </span>
                                {reference.approved ? (
                                    <span className="flex shrink-0 items-center gap-1 text-xs text-muted-foreground">
                                        <Check className="h-3.5 w-3.5" />
                                        已确认
                                    </span>
                                ) : reference.target ? (
                                    <span className="flex shrink-0 items-center gap-2">
                                        <span className="flex items-center gap-1 text-xs text-muted-foreground">
                                            <AlertTriangle className="h-3.5 w-3.5" />
                                            配置已变化
                                        </span>
                                        <Button type="button" variant="outline" size="sm" className="h-7"
                                            aria-label={`确认 ${reference.name} 使用此凭据访问 ${reference.target}`}
                                            onClick={() => onConfirmReference(reference)}>
                                            确认
                                        </Button>
                                    </span>
                                ) : null}
                            </li>
                        ))}
                    </ul>
                )}
            </Section>

            <Section title="最近使用">
                {usage.length === 0 ? (
                    <p className="text-sm text-muted-foreground">还没有被使用过</p>
                ) : (
                    <ul className="divide-y divide-border rounded-lg border border-border text-sm">
                        {usage.map((entry) => (
                            <li key={entry.id} className="flex items-center gap-3 px-3 py-2" title={entry.reason ?? undefined}>
                                <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{formatTime(entry.createdAt)}</span>
                                <span className="shrink-0 font-mono text-xs">{entry.method}</span>
                                <span className="min-w-0 flex-1 truncate font-mono text-xs">{entry.url}</span>
                                <span className={entry.outcome === "ok" ? "shrink-0 text-xs" : "shrink-0 text-xs text-muted-foreground"}>
                                    {outcomeText(entry)}
                                </span>
                            </li>
                        ))}
                    </ul>
                )}
            </Section>
        </div>
    );
}
