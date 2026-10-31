import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useDataRefresh } from "@/hooks/useDataRefresh";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { previewMobile } from "./navigation";
import { usePlatform } from "@/hooks/use-platform";

interface Collection { id: number; name: string; artifact_type: string; code: string }
export default function MobileCollections() {
    const platform = usePlatform();
    const [items, setItems] = useState<Collection[]>([]);
    const [query, setQuery] = useState("");
    const [error, setError] = useState("");
    const [loading, setLoading] = useState(true);
    const [deleting, setDeleting] = useState<number | null>(null);
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
    const visibleItems = items.filter(item => item.name.toLowerCase().includes(query.toLowerCase()));
    return <section className={`h-full flex flex-col safe-top safe-bottom bg-background ${platform === "android" ? "safe-top-android" : ""}`}>
        <header className="flex items-center gap-3 p-3"><Button className="min-h-11" variant="ghost" onClick={() => window.__aippMobileBack?.()}>返回</Button><h1>收藏</h1></header>
        <Input aria-label="搜索收藏" placeholder="搜索收藏" value={query} onChange={e => setQuery(e.target.value)} />
        {error && <p role="alert">{error}</p>}
        <div className="overflow-auto flex-1 p-3 space-y-3">
            {loading ? <p role="status">加载中…</p> : items.length === 0 ? <p>暂无收藏</p> : null}
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
