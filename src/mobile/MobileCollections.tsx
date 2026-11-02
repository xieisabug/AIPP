import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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
            previewMobile({ language: item.artifact_type, code: item.code, title: item.name });
        } catch (e) { setError(`打开收藏 ${id} 失败：${String(e)}`); }
    };
    const remove = async (id: number) => {
        try { await invoke("delete_artifact_collection", { id }); setDeleting(null); await refresh(); }
        catch (e) { setError(`删除收藏 ${id} 失败：${String(e)}`); }
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
    return <section className={`h-full flex flex-col safe-top safe-bottom bg-background ${platform === "android" ? "safe-top-android" : ""}`}>
        <header className="flex items-center gap-3 p-3">
            <Button className="min-h-11" variant="ghost" onClick={() => window.__aippMobileBack?.()}>返回</Button>
            <h1 className="flex-1">收藏</h1>
            {!creating ? (
                <Button className="min-h-11" onClick={() => setCreating(true)}>新建收藏</Button>
            ) : null}
        </header>
        <Input aria-label="搜索收藏" placeholder="搜索收藏" value={query} onChange={e => setQuery(e.target.value)} />
        {error && <p role="alert">{error}</p>}
        {creating ? (
            <form
                className="p-3 space-y-3 border-b"
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
                        <SelectTrigger aria-label="收藏类型" className="min-h-11">
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
        <div className="overflow-auto flex-1 p-3 space-y-3">
            {loading ? <p role="status">加载中…</p> : null}
            {!loading && items.length === 0 && !creating ? (
                <div className="space-y-3">
                    <p>暂无收藏</p>
                    <Button className="min-h-11" onClick={() => setCreating(true)}>新建收藏</Button>
                </div>
            ) : null}
            {!loading && items.length > 0 && visibleItems.length === 0 && <p>没有匹配的收藏</p>}
            {visibleItems.map(item => <article className="border rounded p-3" key={item.id}>
                <Button variant="ghost" className="min-h-11 max-w-full whitespace-normal" onClick={() => void open(item.id)}>{item.name}</Button>
                <span className="text-muted-foreground text-sm">{item.artifact_type}</span>
                {deleting === item.id ? <div><span>确认删除此收藏？</span><Button className="min-h-11" onClick={() => void remove(item.id)}>确认删除</Button><Button className="min-h-11" variant="outline" onClick={() => setDeleting(null)}>取消</Button></div>
                    : <Button className="min-h-11" variant="ghost" onClick={() => setDeleting(item.id)}>删除</Button>}
            </article>)}
        </div>
    </section>;
}
