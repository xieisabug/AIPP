import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowDownToLine, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";

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

    return (
        <>
            <Button variant="outline" size="sm" onClick={() => setOpen(true)}>
                <ArrowDownToLine className="h-4 w-4" />
                迁移明文
            </Button>
            <Dialog open={open} onOpenChange={(next) => { if (!busy) setOpen(next); }}>
                <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl">
                    <DialogHeader>
                        <DialogTitle>迁移明文凭据</DialogTitle>
                        <DialogDescription>选择要迁入保险箱的配置。每项确认后先加密保存，再将原配置替换为引用。</DialogDescription>
                    </DialogHeader>
                    {loading ? (
                        <p className="py-8 text-center text-sm text-muted-foreground">正在扫描配置…</p>
                    ) : scanError ? (
                        <div className="space-y-3 py-4">
                            <p role="alert" className="text-sm text-destructive">{scanError}</p>
                            <Button variant="outline" size="sm" onClick={() => void scan()}><RefreshCw className="h-4 w-4" />重新扫描</Button>
                        </div>
                    ) : candidates.length === 0 ? (
                        <p className="py-8 text-center text-sm text-muted-foreground">没有发现可迁移的明文凭据。</p>
                    ) : (
                        <div className="max-h-[50vh] space-y-1 overflow-y-auto">
                            {candidates.map((candidate) => {
                                const id = identity(candidate);
                                return (
                                    <label key={id} className="flex cursor-pointer items-start gap-3 border-b border-border px-2 py-3 hover:bg-muted/40">
                                        <Checkbox checked={selected.has(id)} onCheckedChange={(checked) => {
                                            setSelected((current) => {
                                                const next = new Set(current);
                                                if (checked === true) next.add(id);
                                                else next.delete(id);
                                                return next;
                                            });
                                        }} aria-label={`迁移 ${candidate.label}`} />
                                        <span className="min-w-0 flex-1">
                                            <span className="block truncate text-sm font-medium">{candidate.label}</span>
                                            <span className="mt-1 block break-all font-mono text-xs text-muted-foreground">{candidate.field}</span>
                                            <span className="mt-1 block text-xs text-muted-foreground">凭据目标：{candidate.target}</span>
                                        </span>
                                    </label>
                                );
                            })}
                        </div>
                    )}
                    {results.length > 0 && <div aria-live="polite" className="space-y-1 text-xs">{results.map((result) => <p key={result}>{result}</p>)}</div>}
                    <DialogFooter>
                        <Button variant="outline" onClick={() => void scan()} disabled={loading || busy || candidates.length === 0}>重新扫描</Button>
                        <Button onClick={() => void migrateSelected()} disabled={loading || busy || selectedCandidates.length === 0}>
                            {busy ? "正在迁移…" : `确认迁移 ${selectedCandidates.length} 项`}
                        </Button>
                    </DialogFooter>
                </DialogContent>
            </Dialog>
        </>
    );
}
