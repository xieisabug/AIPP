import { GripVertical } from "lucide-react";
import { useState, type PointerEvent, type ReactNode } from "react";

interface Row { id: string; label: string; content: ReactNode }

/** The chief stays outside this list. Only an explicit move writes the local preference. */
export default function ButlerSortableList({ rows, storageKey }: { rows: Row[]; storageKey: string }) {
    const [saved, setSaved] = useState<string[]>(() => {
        try {
            const ids: unknown = JSON.parse(localStorage.getItem(storageKey) ?? "[]");
            return Array.isArray(ids) ? ids.filter((id): id is string => typeof id === "string") : [];
        } catch { return []; }
    });
    const [target, setTarget] = useState<{ id: string; after: boolean } | null>(null);
    const byId = new Map(rows.map(row => [row.id, row]));
    const ordered = [...new Set([...saved.filter(id => byId.has(id)), ...byId.keys()])];

    function move(id: string, to: string, after: boolean) {
        if (id === to || !byId.has(id) || !byId.has(to)) return;
        const next = ordered.filter(key => key !== id);
        next.splice(next.indexOf(to) + Number(after), 0, id);
        setSaved(next);
        try { localStorage.setItem(storageKey, JSON.stringify(next)); } catch { /* Keep the session order. */ }
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
            <button type="button" aria-label={`调整顺序：${row.label}`}
                title="拖动排序，也可用上下方向键移动"
                className="shrink-0 touch-none cursor-grab rounded px-0.5 py-2 text-muted-foreground opacity-0 hover:bg-muted group-hover/sort:opacity-100 focus-visible:opacity-100 active:cursor-grabbing [@media(hover:none)]:opacity-100"
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
