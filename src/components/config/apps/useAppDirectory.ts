import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { PluginSource } from './AgentPluginsConfig';
import type { OfficialIcon } from './AppIcon';

export interface DirectoryCard extends OfficialIcon {
    id: string; kind: 'app' | 'plugin'; name: string; description: string; publisher: string; group: string; source?: PluginSource;
}
interface Group { id: string; name: string; total: number; items: DirectoryCard[] }
interface Home { version: string; categories: Group[] }
interface Page { version: string; items: DirectoryCard[]; next_cursor: number | null }
interface Reply<T> { data?: T; etag?: string; changed?: boolean; unchanged?: boolean }
interface Cached { data: Home; time: number; etag?: string }
const pending = new Map<string, Promise<Reply<Home>>>();
function cached(key: string): Cached | undefined {
    try {
        const value = JSON.parse(localStorage.getItem(key) || 'null');
        return Array.isArray(value?.data?.categories) ? value : undefined;
    } catch { return undefined; }
}

export function useAppDirectory(nativeOnly: boolean, category: string, query: string, enabled: boolean, refresh: number) {
    const key = 'aipp.directory.v2.' + nativeOnly;
    const [home, setHome] = useState<Home>();
    const [items, setItems] = useState<DirectoryCard[]>([]);
    const [homeError, setHomeError] = useState('');
    const [pageError, setPageError] = useState('');
    const [homeBusy, setHomeBusy] = useState(false);
    const [pageBusy, setPageBusy] = useState(false);
    const pages = useRef(new Map<string,Page>());
    const [next, setNext] = useState<number | null>(null);
    const load = useRef<() => void>(() => {});
    const anchor = useRef<HTMLDivElement>(null);
    const [revision, setRevision] = useState(0);
    const refreshed = useRef({refresh:0,revision:0});
    const list = Boolean(category || query.trim());

    useEffect(() => {
        if (!enabled) return;
        let active = true;
        const saved = cached(key);
        setHome(saved?.data); setHomeError('');
        const forced = refreshed.current.refresh !== refresh || refreshed.current.revision !== revision;
        refreshed.current = {refresh,revision};
        if (saved && !forced && Date.now()-saved.time >= 0 && Date.now()-saved.time < 300_000) { setHomeBusy(false); return; }
        setHomeBusy(true);
        let request = pending.get(key);
        if (!request) {
            request = invoke<Reply<Home>>('fetch_app_directory', {nativeOnly,paged:false,etag:saved?.etag ?? null}).finally(()=>pending.delete(key));
            pending.set(key,request);
        }
        void request.then(reply => {
            const data = reply.unchanged ? saved?.data : reply.data;
            if (!data || !Array.isArray(data.categories)) throw new Error('应用分类目录格式无效');
            try { localStorage.setItem(key,JSON.stringify({data,etag:reply.etag ?? saved?.etag,time:Date.now()})); } catch { /* Storage may be unavailable. */ }
            if (active) setHome(data);
        }).catch(e=>{ if (active) setHomeError(String(e)); }).finally(()=>{ if (active) setHomeBusy(false); });
        return () => { active=false; };
    },[enabled,key,nativeOnly,refresh,revision]);

    useEffect(() => {
        let active=true, busy=false, cursor:number|null=0, version=home?.version;
        setItems([]); setNext(null); setPageError(''); setPageBusy(false);
        load.current=()=>{};
        if (!enabled || !list) return;
        const fetchNext=async () => {
            if (!active || busy || cursor===null) return;
            busy=true; setPageBusy(true); setPageError('');
            try {
                const args={nativeOnly,paged:true,category:category||null,query:query.trim()||null,cursor,version:version??null};
                const cacheKey=JSON.stringify(args);
                const cachedPage=version ? pages.current.get(cacheKey) : undefined;
                const result=cachedPage ? {data:cachedPage,changed:false} : await invoke<Reply<Page>>('fetch_app_directory',args);
                if (!active) return;
                if (result.changed) {
                    setRevision(v=>v+1);
                    throw new Error('应用目录已更新，正在刷新分类。');
                }
                const page=result.data;
                if (!Array.isArray(page?.items)) throw new Error('分类列表格式无效');
                if (page.next_cursor!==null && (!Number.isSafeInteger(page.next_cursor) || page.next_cursor<=cursor)) throw new Error('分类分页游标无效');
                version=page.version;
                if (!cachedPage && args.version) {
                    if (pages.current.size>=64) pages.current.delete(pages.current.keys().next().value!);
                    pages.current.set(cacheKey,page);
                }
                cursor=page.next_cursor;
                setNext(cursor); setItems(old=>[...old,...page.items]);
            } catch(e) { if(active) setPageError(String(e)); }
            finally { busy=false; if(active) setPageBusy(false); }
        };
        load.current=fetchNext;
        const timer=window.setTimeout(fetchNext,query.trim()?220:0);
        return ()=>{active=false;window.clearTimeout(timer);};
    },[enabled,list,nativeOnly,category,query,home?.version,refresh]);

    useEffect(()=>{
        if (!anchor.current || next===null || pageBusy || pageError || !('IntersectionObserver' in window)) return;
        const observer=new IntersectionObserver(entries=>{if(entries.some(e=>e.isIntersecting))load.current();},{rootMargin:'240px'});
        observer.observe(anchor.current);return ()=>observer.disconnect();
    },[next,pageBusy,pageError]);
    const more=useCallback(()=>load.current(),[]);
    return {home,items,anchor,next,more,homeError,pageError,homeBusy,pageBusy};
}
