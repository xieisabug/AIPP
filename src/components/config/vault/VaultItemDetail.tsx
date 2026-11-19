import { useEffect, useState } from "react";
import { AlertTriangle, Check, Copy, KeyRound, Pencil, Plus, Trash2, X } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { vaultPlaceholder, type VaultConfigReference, type VaultItem, type VaultUsageEntry } from "@/data/Vault";
import VaultItemEditDialog from "./VaultItemEditDialog";
import { formatDateTime, formatRelative, formatTime } from "./vaultHelpers";

export interface VaultItemPatch {
    name?: string;
    key?: string;
    description?: string;
    allowedSites?: string[];
    syncEnabled?: boolean;
}

interface VaultItemDetailProps {
    item: VaultItem;
    usage: VaultUsageEntry[];
    takenKeys: string[];
    syncReady: boolean;
    /** 引用此条目的 MCP / Provider 配置及其当前目标。 */
    references: VaultConfigReference[];
    onConfirmReference: (reference: VaultConfigReference) => void;
    onUpdate: (patch: VaultItemPatch) => Promise<boolean>;
    onReplace: () => void;
    onDelete: () => void;
}

/** 使用记录默认只展开几条，其余按需显示。 */
const USAGE_PREVIEW = 5;

function outcomeText(entry: VaultUsageEntry): string {
    if (entry.outcome === "ok") return "完成";
    if (entry.outcome === "denied") return "已拒绝";
    return "失败";
}

function Section({ slot, title, description, children }: { slot: string; title: string; description?: React.ReactNode; children: React.ReactNode }) {
    return (
        <section className="space-y-3" data-aipp-slot={slot}>
            <div className="space-y-1">
                <h3 className="text-sm font-medium">{title}</h3>
                {description && <p className="text-xs leading-relaxed text-muted-foreground">{description}</p>}
            </div>
            {children}
        </section>
    );
}

/** 「基本信息」里的一行：标签 · 内容 · 可选操作。 */
function Field({ label, children, action }: { label: string; children: React.ReactNode; action?: React.ReactNode }) {
    return (
        <div className="flex min-h-12 items-center gap-4 px-4 py-2.5">
            <dt className="w-24 shrink-0 text-sm text-muted-foreground">{label}</dt>
            <dd className="min-w-0 flex-1 text-sm">{children}</dd>
            {action && <div className="shrink-0">{action}</div>}
        </div>
    );
}

/** 列表框里没有内容时的一行说明，保持各区块形状一致。 */
function EmptyRow({ children }: { children: React.ReactNode }) {
    return <li className="px-4 py-3 text-sm text-muted-foreground">{children}</li>;
}

export default function VaultItemDetail({ item, usage, takenKeys, syncReady, references, onConfirmReference, onUpdate, onReplace, onDelete }: VaultItemDetailProps) {
    const [editOpen, setEditOpen] = useState(false);
    const [addingSite, setAddingSite] = useState(false);
    const [newSite, setNewSite] = useState("");
    const [showAllUsage, setShowAllUsage] = useState(false);

    useEffect(() => {
        setEditOpen(false);
        setAddingSite(false);
        setNewSite("");
        setShowAllUsage(false);
    }, [item.id]);

    const placeholder = vaultPlaceholder(item.key);
    const locked = item.status === "locked";
    const syncText = !item.syncEnabled ? "只保存在本机" : syncReady ? "同步到我的其他设备（端到端加密）" : "已开启，设置同步口令后开始同步";
    const visibleUsage = showAllUsage ? usage : usage.slice(0, USAGE_PREVIEW);

    const copyPlaceholder = async () => {
        try {
            await navigator.clipboard.writeText(placeholder);
            toast.success("已复制占位符");
        } catch {
            toast.error("复制失败");
        }
    };

    const closeAddSite = () => {
        setAddingSite(false);
        setNewSite("");
    };

    const addSite = async () => {
        const site = newSite.trim();
        if (!site) return;
        if (await onUpdate({ allowedSites: [...item.allowedSites, site] })) closeAddSite();
    };

    return (
        <Card className="gap-0 py-0 shadow-none" data-aipp-slot="vault-detail">
            <CardHeader className="flex flex-row items-start justify-between gap-4 border-b py-5" data-aipp-slot="vault-detail-header">
                <div className="min-w-0 space-y-2">
                    <div className="flex items-center gap-2">
                        <h2 className="truncate text-lg font-semibold">{item.name}</h2>
                        {locked && <Badge variant="outline" className="shrink-0">待解锁</Badge>}
                    </div>
                    <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
                        <button
                            type="button"
                            className="inline-flex items-center gap-1 rounded border border-border bg-muted/50 px-1.5 py-0.5 text-foreground hover:bg-muted"
                            aria-label={`复制占位符 ${placeholder}`}
                            title="复制"
                            onClick={() => void copyPlaceholder()}
                        >
                            <code className="font-mono text-xs">{placeholder}</code>
                            <Copy className="h-3 w-3 text-muted-foreground" />
                        </button>
                        助手在工具调用里写这个占位符来使用它，每次使用都会先问你
                    </p>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                    <Button variant="ghost" size="sm" onClick={() => setEditOpen(true)}>
                        <Pencil className="mr-1 h-4 w-4" />
                        编辑
                    </Button>
                    <Button variant="ghost" size="sm" onClick={onDelete}>
                        <Trash2 className="mr-1 h-4 w-4" />
                        删除
                    </Button>
                </div>
            </CardHeader>

            <CardContent className="space-y-8 py-6">
                <Section slot="vault-section-basic" title="基本信息">
                    <dl className="divide-y divide-border rounded-lg border border-border">
                        <Field label="值" action={<Button variant="outline" size="sm" onClick={onReplace}>替换值</Button>}>
                            <span className="mr-3 font-mono tracking-widest text-muted-foreground" aria-hidden>●●●●●●●●</span>
                            <span className="text-xs text-muted-foreground">
                                {locked ? "来自其他设备，输入同步口令后可用" : "已加密保存，不可查看"}
                            </span>
                        </Field>
                        <Field label="给助手的说明">
                            {item.description || <span className="text-muted-foreground">未填写。助手靠名称、键和说明判断该用哪个条目</span>}
                        </Field>
                        <Field label="同步">{syncText}</Field>
                        <Field label="创建时间"><span className="tabular-nums">{formatDateTime(item.createdAt)}</span></Field>
                        <Field label="最后使用">
                            {item.lastUsedAt ? (
                                <span className="tabular-nums">
                                    {formatDateTime(item.lastUsedAt)}
                                    <span className="ml-2 text-muted-foreground">{formatRelative(item.lastUsedAt)}</span>
                                </span>
                            ) : (
                                <span className="text-muted-foreground">从未使用</span>
                            )}
                        </Field>
                    </dl>
                </Section>

                <Section slot="vault-section-sites" title="自动允许的网站" description="助手把这个值发往这些网站时不再询问；定时任务等无人值守的运行只能发往这里列出的网站。本机命令和程序每次都会问你。">
                    <ul className="divide-y divide-border rounded-lg border border-border">
                        {item.allowedSites.length === 0 && !addingSite && (
                            <EmptyRow>还没有。审批时选「总是允许」，网站会自动加到这里。</EmptyRow>
                        )}
                        {item.allowedSites.map((site) => (
                            <li key={site} className="flex min-h-11 items-center justify-between gap-2 py-1.5 pl-4 pr-2">
                                <span className="truncate font-mono text-sm">{site}</span>
                                <Button
                                    variant="ghost"
                                    size="icon"
                                    className="h-8 w-8"
                                    aria-label={`移除 ${site}`}
                                    onClick={() => onUpdate({ allowedSites: item.allowedSites.filter((s) => s !== site) })}
                                >
                                    <X className="h-4 w-4" />
                                </Button>
                            </li>
                        ))}
                        <li className="px-2 py-1.5">
                            {addingSite ? (
                                <div className="flex gap-2 px-2 py-0.5">
                                    <Input
                                        autoFocus
                                        aria-label="添加自动允许的网站"
                                        placeholder="https://api.example.com"
                                        className="h-8 text-sm"
                                        value={newSite}
                                        onChange={(event) => setNewSite(event.target.value)}
                                        onKeyDown={(event) => {
                                            if (event.key === "Enter" && !event.nativeEvent.isComposing) {
                                                event.preventDefault();
                                                void addSite();
                                            } else if (event.key === "Escape") {
                                                event.preventDefault();
                                                closeAddSite();
                                            }
                                        }}
                                    />
                                    <Button size="sm" className="h-8" disabled={!newSite.trim()} onClick={() => void addSite()}>添加</Button>
                                    <Button variant="ghost" size="sm" className="h-8" onClick={closeAddSite}>取消</Button>
                                </div>
                            ) : (
                                <Button variant="ghost" size="sm" className="h-8 gap-1 text-muted-foreground" onClick={() => setAddingSite(true)}>
                                    <Plus className="h-4 w-4" />
                                    添加网站
                                </Button>
                            )}
                        </li>
                    </ul>
                </Section>

                <Section
                    slot="vault-section-references"
                    title="配置引用"
                    description={
                        <>
                            在 Provider 的 API Key、MCP 的请求头或环境变量旁点
                            <KeyRound className="mx-1 inline h-3 w-3 align-[-1px]" aria-label="钥匙" />
                            选择这个条目，配置里只保存引用。每个配置只会把值发往你确认过的地址。
                        </>
                    }
                >
                    <ul className="divide-y divide-border rounded-lg border border-border text-sm" data-aipp-slot="vault-references">
                        {references.length === 0 && <EmptyRow>还没有配置引用它。</EmptyRow>}
                        {references.map((reference) => (
                            <li key={`${reference.kind}-${reference.configId}`} className="flex min-h-11 items-center gap-4 py-1.5 pl-4 pr-2">
                                <span className="max-w-[45%] shrink-0 truncate" title={reference.name}>{reference.name}</span>
                                <span className="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground" title={reference.target ?? reference.problem ?? undefined}>
                                    {reference.target ?? reference.problem}
                                </span>
                                {reference.approved ? (
                                    <span className="flex shrink-0 items-center gap-1 pr-2 text-xs text-muted-foreground">
                                        <Check className="h-3.5 w-3.5" />
                                        已确认
                                    </span>
                                ) : reference.target ? (
                                    <span className="flex shrink-0 items-center gap-2">
                                        <span className="flex items-center gap-1 text-xs">
                                            <AlertTriangle className="h-3.5 w-3.5" />
                                            配置已变化
                                        </span>
                                        <Button type="button" size="sm" className="h-8"
                                            aria-label={`确认 ${reference.name} 使用此条目访问 ${reference.target}`}
                                            onClick={() => onConfirmReference(reference)}>
                                            确认
                                        </Button>
                                    </span>
                                ) : null}
                            </li>
                        ))}
                    </ul>
                </Section>

                <Section slot="vault-section-usage" title="使用记录" description="最近 20 次，只保存在本机，不会同步。">
                    <ul className="divide-y divide-border rounded-lg border border-border text-sm">
                        {usage.length === 0 && <EmptyRow>还没有使用记录。</EmptyRow>}
                        {visibleUsage.map((entry) => (
                            <li key={entry.id} className="flex min-h-11 items-center gap-4 px-4 py-2" title={entry.reason ?? undefined}>
                                <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{formatTime(entry.createdAt)}</span>
                                <span className="min-w-0 flex-1 truncate" title={entry.destination}>{entry.destination}</span>
                                <span className="hidden max-w-[30%] shrink-0 truncate text-xs text-muted-foreground lg:block" title={entry.tool}>{entry.tool}</span>
                                <span className={entry.outcome === "ok" ? "w-12 shrink-0 text-right text-xs text-muted-foreground" : "w-12 shrink-0 text-right text-xs font-medium"}>
                                    {outcomeText(entry)}
                                </span>
                            </li>
                        ))}
                    </ul>
                    {usage.length > USAGE_PREVIEW && (
                        <Button variant="ghost" size="sm" className="h-7 px-2 text-xs text-muted-foreground" onClick={() => setShowAllUsage((value) => !value)}>
                            {showAllUsage ? "收起" : `显示全部 ${usage.length} 条`}
                        </Button>
                    )}
                </Section>
            </CardContent>

            <VaultItemEditDialog
                item={item}
                open={editOpen}
                takenKeys={takenKeys}
                syncReady={syncReady}
                onClose={() => setEditOpen(false)}
                onUpdate={onUpdate}
            />
        </Card>
    );
}
