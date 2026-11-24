import { GripVertical } from "lucide-react";
import { useRef, useState, type PointerEvent, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { useDataRefresh } from "@/hooks/useDataRefresh";

interface Row { id: string; label: string; content: ReactNode }

const legacyKey = "butler-contact-order";
function readLegacy(): string[] {
    try {
        const ids: unknown = JSON.parse(localStorage.getItem(legacyKey) ?? "[]");
        return Array.isArray(ids) ? ids.filter((id): id is string => typeof id === "string" && /^(assistant|group):[1-9]\d*$/.test(id)) : [];
    } catch { return []; }
}

export default function ButlerSortableList({ rows }: { rows: Row[] }) {
    const [saved, setSaved] = useState<string[]>(readLegacy);
    const [loaded, setLoaded] = useState(false);
    const writes = useRef(Promise.resolve());
    const revision = useRef(0);
    const refresh = useDataRefresh({
        events: ["feature_config_changed", "assistant_list_changed", "conversation_data_changed"],
        initial: true,
        refresh: async isCurrent => {
            const version = revision.current;
            await writes.current;
            const legacy = readLegacy();
            const order = await invoke<string[]>("get_butler_contact_order", { legacy });
            if (!isCurrent() || version !== revision.current) return;
            setSaved(order);
            setLoaded(true);
            if (legacy.length) localStorage.removeItem(legacyKey);
        },
    });
    const [target, setTarget] = useState<{ id: string; after: boolean } | null>(null);
    const byId = new Map(rows.map(row => [row.id, row]));
    const ordered = [...new Set([...saved.filter(id => byId.has(id)), ...byId.keys()])];

    function move(id: string, to: string, after: boolean) {
        if (id === to || !byId.has(id) || !byId.has(to)) return;
        const next = ordered.filter(key => key !== id);
        next.splice(next.indexOf(to) + Number(after), 0, id);
        if (next.every((key, index) => key === ordered[index])) return;
        const version = ++revision.current;
        setSaved(next);
        writes.current = writes.current.then(() => invoke<void>("save_butler_contact_order", { order: next }))
            .catch(error => toast.error(`保存联系人顺序失败：${String(error)}`))
            .then(() => { if (version === revision.current) refresh(); });
    }

    function destination(event: PointerEvent<HTMLButtonElement>) {
        const row = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>("[data-butler-sort-id]");
        if (!row || row.parentElement !== event.currentTarget.parentElement?.parentElement) return null;
        const id = row.dataset.butlerSortId!;
        const source = event.currentTarget.parentElement?.dataset.butlerSortId;
        if (!source || id === source) return null;
        const rect = row.getBoundingClientRect();
        return { id, after: event.clientY > rect.top + rect.height / 2 };
    }

    return <div>{ordered.map((id, index) => {
        const row = byId.get(id)!;
        return <div key={id} data-butler-sort-id={id} className="group/sort relative flex items-center">
            <div className="min-w-0 flex-1">{row.content}</div>
            {ordered.length > 1 &&
            <button type="button" disabled={!loaded} aria-label={`调整顺序：${row.label}`}
                title="拖动排序，也可用上下方向键移动"
                className="aipp-icon-button shrink-0 touch-none cursor-grab rounded px-0.5 py-2 text-muted-foreground opacity-0 hover:bg-muted group-hover/sort:opacity-100 focus-visible:opacity-100 active:cursor-grabbing [@media(hover:none)]:opacity-100"
                onPointerDown={event => { if (event.button === 0 && event.isPrimary) event.currentTarget.setPointerCapture(event.pointerId); }}
                onPointerMove={event => {
                    if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
                    const nav = event.currentTarget.closest("nav");
                    if (nav) {
                        const rect = nav.getBoundingClientRect();
                        if (event.clientY < rect.top + 24) nav.scrollTop -= 16;
                        else if (event.clientY > rect.bottom - 24) nav.scrollTop += 16;
                    }
                    setTarget(destination(event));
                }}
                onPointerUp={event => {
                    if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
                    const drop = destination(event);
                    if (drop) move(id, drop.id, drop.after);
                    setTarget(null);
                }}
                onLostPointerCapture={() => setTarget(null)}
                onKeyDown={event => {
                    const delta = event.key === "ArrowUp" ? -1 : event.key === "ArrowDown" ? 1 : 0;
                    if (!delta) return;
                    event.preventDefault();
                    if (ordered[index + delta]) move(id, ordered[index + delta], delta > 0);
                }}><GripVertical size={14}/></button>}
            {target?.id === id && <span className={`pointer-events-none absolute inset-x-0 h-0.5 bg-primary ${target.after ? "bottom-0" : "top-0"}`}/>}
        </div>;
    })}</div>;
}
