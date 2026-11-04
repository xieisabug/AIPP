import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronLeft, X } from "lucide-react";
import { useDataRefresh } from "@/hooks/useDataRefresh";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { previewMobile } from "./navigation";
import { usePlatform } from "@/hooks/use-platform";
import { getDefaultIcon } from "@/utils/emojiUtils";

interface Collection { id: number; name: string; artifact_type: string; code: string }

const CREATE_TYPES = ["markdown", "html", "text", "svg"] as const;

export default function MobileCollections() {
    const platform = usePlatform();
    const [items, setItems] = useState<Collection[]>([]);
    const [query, setQuery] = useState("");
    const [error, setError] = useState("");
    const [loading, setLoading] = useState(true);
    const [deleting, setDeleting] = useState<number | null>(null);
    const [renaming, setRenaming] = useState<number | null>(null);
    const [renameValue, setRenameValue] = useState("");
    const [creating, setCreating] = useState(false);
    const [saving, setSaving] = useState(false);
    const [name, setName] = useState("");
    const [artifactType, setArtifactType] = useState<string>("markdown");
    const [code, setCode] = useState("");
    const refresh = async () => setItems(await invoke<Collection[]>("get_artifacts_collection", { artifactType: null }));
    useDataRefresh({ events: ["artifact-collection-updated", "artifacts_collection_changed"], initial: true,
        refresh: async () => { try { await refresh(); setError(""); } finally { setLoading(false); } },
        onError: e => setError(`加载收藏失败：${String(e)}`) });
    const open = async (id: number) => {
        try {
            const item = await invoke<Collection | null>("get_artifact_by_id", { id });
            if (!item) throw new Error(`收藏 ${id} 已不存在`);
            previewMobile({ language: item.artifact_type, code: item.code, title: item.name, collectionId: item.id });
        } catch (e) { setError(`打开收藏 ${id} 失败：${String(e)}`); }
    };
    const remove = async (id: number) => {
        try { await invoke("delete_artifact_collection", { id }); setDeleting(null); await refresh(); }
        catch (e) { setError(`删除收藏 ${id} 失败：${String(e)}`); }
    };
    const startRename = (item: Collection) => {
        setDeleting(null);
        setRenaming(item.id);
        setRenameValue(item.name);
    };
    const rename = async (id: number) => {
        const trimmed = renameValue.trim();
        if (!trimmed) {
            setError("请输入收藏名称");
            return;
        }
        try {
            await invoke("update_artifact_collection", { request: { id, name: trimmed } });
            setRenaming(null);
            await refresh();
        } catch (e) { setError(`重命名收藏 ${id} 失败：${String(e)}`); }
    };
    const resetCreate = () => {
        setCreating(false);
        setName("");
        setArtifactType("markdown");
        setCode("");
        setSaving(false);
    };
    const create = async () => {
        const trimmedName = name.trim();
        if (!trimmedName) {
            setError("请输入收藏名称");
            return;
        }
        setSaving(true);
        try {
            await invoke<number>("save_artifact_to_collection", {
                request: {
                    name: trimmedName,
                    icon: getDefaultIcon(),
                    description: "",
                    artifact_type: artifactType,
                    code,
                    tags: null,
                    db_id: null,
                    assistant_id: null,
                },
            });
            resetCreate();
            await refresh();
            setError("");
        } catch (e) {
            setError(`创建收藏失败：${String(e)}`);
            setSaving(false);
        }
    };
    const visibleItems = items.filter(item => item.name.toLowerCase().includes(query.toLowerCase()));
    return <section className="h-full flex flex-col bg-background">
        <header className={`flex shrink-0 items-center gap-2 border-b border-border bg-secondary px-2 py-1 safe-top ${platform === "android" ? "safe-top-android" : ""}`}>
            <Button className="min-h-11 gap-1 px-2" variant="ghost" onClick={() => window.__aippMobileBack?.()}>
                <ChevronLeft className="h-5 w-5" />
                返回
            </Button>
            <h1 className="min-w-0 flex-1 truncate text-center text-sm font-medium">收藏</h1>
            {!creating ? (
                <Button className="min-h-11" variant="ghost" onClick={() => setCreating(true)}>新建收藏</Button>
            ) : <span className="min-w-11" aria-hidden />}
        </header>
        {/* The whole page scrolls so the create form's actions stay reachable above the IME. */}
        <div className="min-h-0 flex-1 overflow-y-auto safe-bottom">
            <div className="space-y-3 p-3">
                <Input aria-label="搜索收藏" type="search" enterKeyHint="search" className="min-h-11" placeholder="搜索收藏" value={query} onChange={e => setQuery(e.target.value)} />
                {error && <div role="alert" className="flex items-start gap-2 rounded-md border border-destructive/40 bg-muted px-3 py-2 text-sm text-destructive">
                    <p className="min-w-0 flex-1 break-words py-1">{error}</p>
                    <Button variant="ghost" size="icon" className="-my-1 shrink-0 text-muted-foreground" aria-label="关闭提示" onClick={() => setError("")}>
                        <X className="h-4 w-4" />
                    </Button>
                </div>}
                {creating ? (
                    <form
                        className="space-y-3 rounded-md border border-border p-3"
                        onSubmit={(event) => {
                            event.preventDefault();
                            void create();
                        }}
                    >
                        <div className="space-y-2">
                            <Label htmlFor="mobile-collection-name">名称</Label>
                            <Input
                                id="mobile-collection-name"
                                aria-label="收藏名称"
                                value={name}
                                onChange={(event) => setName(event.target.value)}
                                placeholder="输入收藏名称"
                                autoFocus
                            />
                        </div>
                        <div className="space-y-2">
                            <Label>类型</Label>
                            <Select value={artifactType} onValueChange={setArtifactType}>
                                <SelectTrigger aria-label="收藏类型" className="min-h-11 w-full">
                                    <SelectValue placeholder="选择类型" />
                                </SelectTrigger>
                                <SelectContent>
                                    {CREATE_TYPES.map((type) => (
                                        <SelectItem key={type} value={type}>{type}</SelectItem>
                                    ))}
                                </SelectContent>
                            </Select>
                        </div>
                        <div className="space-y-2">
                            <Label htmlFor="mobile-collection-code">内容</Label>
                            <Textarea
                                id="mobile-collection-code"
                                aria-label="收藏内容"
                                value={code}
                                onChange={(event) => setCode(event.target.value)}
                                placeholder="输入要收藏的内容"
                                rows={6}
                            />
                        </div>
                        <div className="flex gap-2">
                            <Button type="submit" className="min-h-11" disabled={saving}>
                                {saving ? "保存中…" : "保存"}
                            </Button>
                            <Button type="button" className="min-h-11" variant="outline" onClick={resetCreate} disabled={saving}>
                                取消
                            </Button>
                        </div>
                    </form>
                ) : null}
                {loading ? <p role="status" className="text-sm text-muted-foreground">加载中…</p> : null}
                {!loading && items.length === 0 && !creating ? (
                    <div className="space-y-3 py-6 text-center">
                        <p className="text-sm text-muted-foreground">暂无收藏</p>
                        <Button className="min-h-11" variant="outline" onClick={() => setCreating(true)}>新建收藏</Button>
                    </div>
                ) : null}
                {!loading && items.length > 0 && visibleItems.length === 0 && <p className="text-sm text-muted-foreground">没有匹配的收藏</p>}
                {visibleItems.map(item => <article className="space-y-2 rounded-md border border-border p-3" key={item.id}>
                    {renaming === item.id ? (
                        <form className="space-y-2" onSubmit={(event) => { event.preventDefault(); void rename(item.id); }}>
                            <Input aria-label="新名称" className="min-h-11" value={renameValue} onChange={event => setRenameValue(event.target.value)} enterKeyHint="done" autoFocus />
                            <div className="flex flex-wrap gap-2">
                                <Button type="submit" className="min-h-11">保存名称</Button>
                                <Button type="button" className="min-h-11" variant="outline" onClick={() => setRenaming(null)}>取消</Button>
                            </div>
                        </form>
                    ) : (
                        <div>
                            <button type="button" className="flex min-h-11 w-full items-center rounded-md px-1 text-left text-sm font-medium break-words hover:bg-muted" onClick={() => void open(item.id)}>
                                {item.name}
                            </button>
                            <span className="px-1 text-xs text-muted-foreground">{item.artifact_type}</span>
                        </div>
                    )}
                    {renaming !== item.id && (deleting === item.id ? <div className="flex flex-wrap items-center justify-end gap-2 border-t border-border pt-2">
                        <span className="mr-auto text-sm">确认删除此收藏？</span>
                        <Button className="min-h-11" variant="destructive" onClick={() => void remove(item.id)}>确认删除</Button>
                        <Button className="min-h-11" variant="outline" onClick={() => setDeleting(null)}>取消</Button>
                    </div> : <div className="flex justify-end gap-3 border-t border-border pt-2">
                        <Button className="min-h-11" variant="ghost" onClick={() => startRename(item)}>重命名</Button>
                        <Button className="min-h-11" variant="ghost" onClick={() => { setRenaming(null); setDeleting(item.id); }}>删除</Button>
                    </div>)}
                </article>)}
            </div>
        </div>
    </section>;
}
