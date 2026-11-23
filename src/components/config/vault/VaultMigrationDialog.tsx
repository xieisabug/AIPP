import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, ArrowDownToLine, Loader2, RefreshCw, ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    CompactDialogFooter,
} from "@/components/ui/dialog";
import { cn } from "@/utils/utils";

interface MigrationCandidate {
    kind: string;
    configId: number;
    field: string;
    label: string;
    target: string;
    suggestedName: string;
    suggestedKey: string;
}

interface MigratedItem {
    id: string;
    name: string;
}

function errorText(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
}

export default function VaultMigrationDialog({ onComplete }: { onComplete: () => void }) {
    const [open, setOpen] = useState(false);
    const [candidates, setCandidates] = useState<MigrationCandidate[]>([]);
    const [selected, setSelected] = useState<Set<string>>(new Set());
    const [loading, setLoading] = useState(false);
    const [busy, setBusy] = useState(false);
    const [scanError, setScanError] = useState<string | null>(null);
    const [results, setResults] = useState<string[]>([]);

    const identity = (candidate: MigrationCandidate) => `${candidate.kind}:${candidate.configId}:${candidate.field}`;
    const selectedCandidates = useMemo(() => candidates.filter((candidate) => selected.has(identity(candidate))), [candidates, selected]);

    const scan = async () => {
        setLoading(true);
        setScanError(null);
        setResults([]);
        try {
            const next = await invoke<MigrationCandidate[]>("vault_migration_candidates");
            setCandidates(next);
            setSelected(new Set());
        } catch (error) {
            setScanError(errorText(error));
        } finally {
            setLoading(false);
        }
    };

    useEffect(() => {
        if (open) void scan();
    }, [open]);

    const migrateSelected = async () => {
        if (busy || selectedCandidates.length === 0) return;
        setBusy(true);
        setResults([]);
        const completed: string[] = [];
        const failed: string[] = [];
        for (const candidate of selectedCandidates) {
            try {
                const item = await invoke<MigratedItem>("vault_migrate_candidate", {
                    candidate,
                    name: candidate.suggestedName,
                    key: candidate.suggestedKey,
                });
                completed.push(`已迁移 ${item.name}`);
            } catch (error) {
                failed.push(`${candidate.label}：${errorText(error)}`);
            }
        }
        setResults([...completed, ...failed]);
        if (completed.length > 0) onComplete();
        setBusy(false);
        if (failed.length === 0) setOpen(false);
        else await scan();
    };

    const subtitle = loading
        ? "扫描中…"
        : scanError
            ? undefined
            : candidates.length > 0
                ? `发现 ${candidates.length} 项`
                : "无需迁移";

    return (
        <>
            <Button variant="outline" size="sm" onClick={() => setOpen(true)}>
                <ArrowDownToLine className="h-4 w-4" />
                迁移明文
            </Button>
            <Dialog open={open} onOpenChange={(next) => { if (!busy) setOpen(next); }}>
                <CompactDialogContent className="sm:max-w-2xl" data-aipp-slot="vault-migration-dialog">
                    <CompactDialogHeader
                        title="迁移明文凭据"
                        subtitle={subtitle}
                        description="选择要迁入保险箱的配置。每项确认后先加密保存，再将原配置替换为引用。"
                        closeDisabled={busy}
                    />

                    <CompactDialogBody className="flex flex-col gap-3">
                        <p className="text-xs text-muted-foreground">
                            选择要迁入保险箱的配置。每项确认后先加密保存，再将原配置替换为引用。
                        </p>

                        {loading ? (
                            <div className="flex flex-1 items-center justify-center gap-2 py-10 text-sm text-muted-foreground">
                                <Loader2 className="h-4 w-4 animate-spin" />
                                正在扫描配置…
                            </div>
                        ) : scanError ? (
                            <div role="alert" className="space-y-2 rounded-lg border border-border bg-muted/40 p-3">
                                <div className="flex items-center gap-2 text-sm font-medium">
                                    <AlertTriangle className="h-4 w-4 text-muted-foreground" />
                                    扫描配置失败
                                </div>
                                <p className="break-all text-xs text-destructive">{scanError}</p>
                                <Button variant="outline" size="sm" className="gap-1.5" onClick={() => void scan()}>
                                    <RefreshCw className="h-3.5 w-3.5" />
                                    重新扫描
                                </Button>
                            </div>
                        ) : candidates.length === 0 ? (
                            <div className="flex flex-col items-center gap-2 rounded-lg border border-dashed border-border bg-muted/20 px-4 py-8 text-center">
                                <ShieldCheck className="h-5 w-5 text-muted-foreground" />
                                <p className="text-sm text-muted-foreground">没有发现可迁移的明文凭据。</p>
                            </div>
                        ) : (
                            <>
                                <div className="flex items-center gap-2">
                                    <span className="text-xs text-muted-foreground">
                                        已选 {selectedCandidates.length} / {candidates.length} 项
                                    </span>
                                    <div className="ml-auto flex items-center gap-1">
                                        <Button
                                            type="button"
                                            variant="ghost"
                                            size="sm"
                                            className="h-7 px-2 text-xs"
                                            disabled={busy}
                                            onClick={() => setSelected(new Set(candidates.map(identity)))}
                                        >
                                            全选
                                        </Button>
                                        <Button
                                            type="button"
                                            variant="ghost"
                                            size="sm"
                                            className="h-7 px-2 text-xs"
                                            disabled={busy || selectedCandidates.length === 0}
                                            onClick={() => setSelected(new Set())}
                                        >
                                            清空
                                        </Button>
                                    </div>
                                </div>
                                <div className="min-h-0 flex-1 space-y-1.5 overflow-y-auto pr-0.5">
                                    {candidates.map((candidate) => {
                                        const id = identity(candidate);
                                        const isChecked = selected.has(id);
                                        return (
                                            <label
                                                key={id}
                                                className={cn(
                                                    "flex cursor-pointer items-start gap-2.5 rounded-lg border border-border px-3 py-2.5 transition-colors hover:bg-muted/60",
                                                    isChecked && "border-foreground/40 bg-muted/60",
                                                )}
                                            >
                                                <Checkbox
                                                    className="mt-0.5"
                                                    checked={isChecked}
                                                    disabled={busy}
                                                    onCheckedChange={(checked) => {
                                                        setSelected((current) => {
                                                            const next = new Set(current);
                                                            if (checked === true) next.add(id);
                                                            else next.delete(id);
                                                            return next;
                                                        });
                                                    }}
                                                    aria-label={`迁移 ${candidate.label}`}
                                                />
                                                <span className="min-w-0 flex-1 space-y-1">
                                                    <span className="flex flex-wrap items-center gap-2">
                                                        <span className="truncate text-sm font-medium">{candidate.label}</span>
                                                        <span className="shrink-0 rounded bg-muted px-1.5 py-px font-mono text-[11px] leading-4 text-muted-foreground">
                                                            {candidate.field}
                                                        </span>
                                                    </span>
                                                    <span className="block break-all text-xs text-muted-foreground">凭据目标：{candidate.target}</span>
                                                </span>
                                            </label>
                                        );
                                    })}
                                </div>
                            </>
                        )}

                        {results.length > 0 && (
                            <div aria-live="polite" className="space-y-1 rounded-lg border border-border bg-muted/40 px-3 py-2 text-xs">
                                {results.map((result) => <p key={result}>{result}</p>)}
                            </div>
                        )}
                    </CompactDialogBody>

                    <CompactDialogFooter className="justify-between">
                        <Button
                            type="button"
                            variant="ghost"
                            className="min-h-11 gap-1.5 px-3 sm:min-h-9"
                            onClick={() => void scan()}
                            disabled={loading || busy || candidates.length === 0}
                        >
                            <RefreshCw className="h-3.5 w-3.5" />
                            重新扫描
                        </Button>
                        <div className="ml-auto flex items-center gap-2">
                            <Button
                                type="button"
                                variant="outline"
                                className="min-h-11 px-4 sm:min-h-9"
                                disabled={busy}
                                onClick={() => setOpen(false)}
                            >
                                取消
                            </Button>
                            <Button
                                type="button"
                                className="min-h-11 px-5 sm:min-h-9"
                                onClick={() => void migrateSelected()}
                                disabled={loading || busy || selectedCandidates.length === 0}
                            >
                                {busy && <Loader2 className="h-4 w-4 animate-spin" />}
                                {busy ? "正在迁移…" : `确认迁移 ${selectedCandidates.length} 项`}
                            </Button>
                        </div>
                    </CompactDialogFooter>
                </CompactDialogContent>
            </Dialog>
        </>
    );
}
