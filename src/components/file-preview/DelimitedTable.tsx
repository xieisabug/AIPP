import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { ArrowDown, ArrowUp, Copy, Maximize2, Minimize2, Search, Table2, Text } from "lucide-react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { columnLabel, selectionBounds, type Cell, type SheetProgress, type SheetSlice } from "./delimitedData";
import type { SheetTask } from "./delimited.worker";
import "./delimitedTable.css";

const ROW = 28,
    COL = 142,
    MAX_SCROLL = 16_000_000;
const origin: Cell = { row: 0, col: 0 };
const initial: SheetProgress = { rows: 0, columns: 0, complete: false };
const empty: SheetSlice = { start: 0, rows: [], raw: [] };

function axis(count: number, unit: number, visible: number) {
    const actual = count * unit,
        extent = Math.min(MAX_SCROLL, actual);
    const scale = extent > visible ? (actual - visible) / (extent - visible) : 1;
    return { extent, scale };
}

export default function DelimitedTable({ url, size, delimiter }: { url: string; size: number; delimiter: string }) {
    const root = useRef<HTMLDivElement>(null),
        port = useRef<HTMLDivElement>(null),
        search = useRef<HTMLInputElement>(null);
    const worker = useRef<Worker | null>(null),
        seq = useRef(0);
    const pending = useRef(new Map<number, { ok: (value: unknown) => void; fail: (e: Error) => void }>());
    const [progress, setProgress] = useState(initial),
        [slice, setSlice] = useState(empty),
        [error, setError] = useState("");
    const [viewport, setViewport] = useState({ width: 800, height: 480, x: 0, y: 0 });
    const [anchor, setAnchor] = useState(origin),
        [cell, setCell] = useState(origin),
        [raw, setRaw] = useState(false),
        [page, setPage] = useState(0);
    const [query, setQuery] = useState(""),
        [status, setStatus] = useState(""),
        [fullscreen, setFullscreen] = useState(false);
    const lastFind = useRef(""),
        findId = useRef(0),
        dragging = useRef(false);
    const restoreCell = useRef<Cell | null>(null);
    const send = useCallback(
        <T,>(task: SheetTask): Promise<T> =>
            new Promise((resolve, reject) => {
                if (!worker.current) {
                    reject(new Error("表格预览已关闭"));
                    return;
                }
                const id = ++seq.current;
                pending.current.set(id, { ok: (v) => resolve(v as T), fail: reject });
                worker.current.postMessage({ ...task, id });
            }),
        []
    );
    useEffect(() => {
        const w = new Worker(new URL("./delimited.worker.ts", import.meta.url), { type: "module" });
        let active = true;
        worker.current = w;
        setError("");
        setProgress(initial);
        setSlice(empty);
        w.onmessage = ({ data }) => {
            if (data.progress) {
                setProgress(data.progress);
                return;
            }
            const p = pending.current.get(data.id);
            pending.current.delete(data.id);
            if (data.error) p?.fail(new Error(data.error));
            else p?.ok(data.result);
        };
        w.onerror = (e) => {
            if (active) setError(e.message || "表格后台读取失败");
        };
        void send({ op: "open", url, size, delimiter }).catch((e) => {
            if (active) setError(String(e));
        });
        return () => {
            active = false;
            findId.current++;
            worker.current = null;
            w.terminate();
            for (const p of pending.current.values()) p.fail(new Error("表格预览已关闭"));
            pending.current.clear();
        };
    }, [url, size, delimiter, send]);

    useLayoutEffect(() => {
        const node = port.current;
        if (!node) return;
        node.focus({ preventScroll: true });
        if (restoreCell.current) {
            const { row, col } = restoreCell.current;
            const y = axis(progress.rows, ROW, node.clientHeight);
            const x = axis(progress.columns, COL, node.clientWidth);
            node.scrollTo(
                Math.max(0, (col + 1) * COL - node.clientWidth) / x.scale,
                Math.max(0, (row + 1) * ROW - node.clientHeight) / y.scale
            );
            restoreCell.current = null;
        }
        const resize = () =>
            setViewport((v) => ({
                ...v,
                width: node.clientWidth,
                height: node.clientHeight,
                x: node.scrollLeft,
                y: node.scrollTop,
            }));
        const observer = new ResizeObserver(resize);
        observer.observe(node);
        resize();
        return () => observer.disconnect();
    }, [raw]);
    useEffect(() => {
        const update = () => setFullscreen(document.fullscreenElement === root.current);
        const end = () => {
            dragging.current = false;
        };
        document.addEventListener("fullscreenchange", update);
        window.addEventListener("pointerup", end);
        window.addEventListener("blur", end);
        return () => {
            document.removeEventListener("fullscreenchange", update);
            window.removeEventListener("pointerup", end);
            window.removeEventListener("blur", end);
        };
    }, []);

    const vertical = axis(progress.rows, ROW, viewport.height),
        horizontal = axis(progress.columns, COL, viewport.width);
    const logicalY = viewport.y * vertical.scale,
        logicalX = viewport.x * horizontal.scale;
    const first = raw ? page * 100 : Math.max(0, Math.floor(logicalY / ROW) - 2);
    const count = Math.max(0, Math.min(progress.rows - first, raw ? 100 : Math.ceil(viewport.height / ROW) + 5));
    const firstCol = Math.max(0, Math.floor(logicalX / COL) - 1);
    const cols = Array.from(
        { length: Math.max(0, Math.min(progress.columns - firstCol, Math.ceil(viewport.width / COL) + 3)) },
        (_, i) => i + firstCol
    );
    useEffect(() => {
        let active = true;
        if (count)
            void send<SheetSlice>({ op: "slice", start: first, count }).then(
                (s) => {
                    if (active) setSlice(s);
                },
                (e) => {
                    if (active) setError(String(e));
                }
            );
        return () => {
            active = false;
        };
    }, [first, count, send, url]);
    const select = (next: Cell, extend: boolean, scroll = false) => {
        if (!progress.rows || !progress.columns) return;
        const row = Math.max(0, Math.min(progress.rows - 1, next.row)),
            col = Math.max(0, Math.min(progress.columns - 1, next.col));
        setCell({ row, col });
        if (!extend) setAnchor({ row, col });
        const node = port.current;
        if (scroll && node) {
            const y = Math.max((row + 1) * ROW - node.clientHeight, Math.min(logicalY, row * ROW));
            const x = Math.max((col + 1) * COL - node.clientWidth, Math.min(logicalX, col * COL));
            node.scrollTo(x / horizontal.scale, y / vertical.scale);
        }
    };
    const bounds = selectionBounds(anchor, cell);
    const copy = async () => {
        if (!progress.rows) return;
        try {
            const text = await send<string>({ op: "copy", bounds });
            await writeText(text);
            toast.success("已复制选区");
        } catch (e) {
            toast.error(String(e));
        }
    };
    const find = async (direction: 1 | -1) => {
        if (!query || !progress.complete) return;
        const id = ++findId.current;
        const from =
            lastFind.current === query
                ? cell
                : { row: direction === 1 ? 0 : progress.rows - 1, col: direction === 1 ? -1 : progress.columns };
        lastFind.current = query;
        setStatus("正在查找…");
        try {
            const hit = await send<Cell | null>({ op: "find", query, from, direction });
            if (id !== findId.current) return;
            if (!hit) {
                setStatus("没有匹配项");
                return;
            }
            select(hit, false, true);
            if (raw) setPage(Math.floor(hit.row / 100));
            setStatus(`已定位 ${columnLabel(hit.col)}${hit.row + 1}`);
        } catch (e) {
            if (id === findId.current) setStatus(String(e));
        }
    };
    const keydown = (e: KeyboardEvent<HTMLDivElement>) => {
        const mod = e.ctrlKey || e.metaKey;
        if (mod && e.key.toLowerCase() === "f") {
            e.preventDefault();
            search.current?.focus();
            search.current?.select();
            return;
        }
        if (e.target instanceof HTMLInputElement || raw) return;
        if (mod && e.key.toLowerCase() === "c") {
            e.preventDefault();
            void copy();
            return;
        }
        let { row, col } = cell;
        switch (e.key) {
            case "ArrowDown":
                row++;
                break;
            case "ArrowUp":
                row--;
                break;
            case "ArrowLeft":
                col--;
                break;
            case "ArrowRight":
                col++;
                break;
            case "Home":
                col = 0;
                if (mod) row = 0;
                break;
            case "End":
                col = progress.columns - 1;
                if (mod) row = progress.rows - 1;
                break;
            case "PageUp":
                row -= Math.max(1, Math.floor(viewport.height / ROW));
                break;
            case "PageDown":
                row += Math.max(1, Math.floor(viewport.height / ROW));
                break;
            default:
                return;
        }
        e.preventDefault();
        select({ row, col }, e.shiftKey, true);
    };
    const address = `${columnLabel(cell.col)}${cell.row + 1}`;
    const selectedRow = slice.rows[cell.row - slice.start];
    const [lastVisibleCell, setLastVisibleCell] = useState({ address: "", value: "" });
    const visibleValue = selectedRow ? selectedRow[cell.col] ?? "" : undefined;
    useLayoutEffect(() => {
        if (visibleValue !== undefined) setLastVisibleCell({ address, value: visibleValue });
    }, [address, visibleValue]);
    const value = visibleValue ?? (lastVisibleCell.address === address ? lastVisibleCell.value : "");
    const format = delimiter === "," ? "CSV" : "TSV";
    const toggleFull = async () => {
        try {
            if (fullscreen) await document.exitFullscreen();
            else await root.current?.requestFullscreen();
        } catch (e) {
            toast.error(String(e));
        }
    };
    return (
        <div ref={root} className="aipp-sheet" onKeyDown={keydown}>
            <div className="sheet-tools">
                <div className="sheet-find">
                    <Search size={15} />
                    <input
                        ref={search}
                        aria-label="查找单元格"
                        placeholder="查找单元格"
                        value={query}
                        onChange={(e) => {
                            setQuery(e.target.value);
                            lastFind.current = "";
                            findId.current++;
                            setStatus("");
                        }}
                        onKeyDown={(e) => {
                            if (e.key === "Enter") {
                                e.preventDefault();
                                void find(e.shiftKey ? -1 : 1);
                            }
                        }}
                    />
                    <Button
                        variant="ghost"
                        size="icon"
                        iconLabel="上一个匹配项"
                        disabled={!query || !progress.complete}
                        onClick={() => void find(-1)}
                    >
                        <ArrowUp />
                    </Button>
                    <Button
                        variant="ghost"
                        size="icon"
                        iconLabel="下一个匹配项"
                        disabled={!query || !progress.complete}
                        onClick={() => void find(1)}
                    >
                        <ArrowDown />
                    </Button>
                </div>
                <span className="sheet-find-status" role="status">
                    {status}
                </span>
                <Button
                    variant="ghost"
                    size="icon"
                    iconLabel="复制选区"
                    disabled={!progress.rows || raw}
                    onClick={() => void copy()}
                >
                    <Copy />
                </Button>
                <Button
                    variant="ghost"
                    size="icon"
                    iconLabel={fullscreen ? "退出全屏" : "全屏"}
                    onClick={() => void toggleFull()}
                >
                    {fullscreen ? <Minimize2 /> : <Maximize2 />}
                </Button>
            </div>
            {error ? (
                <p role="alert" className="p-4 text-destructive">
                    {error}
                </p>
            ) : progress.complete && !progress.rows ? (
                <div className="m-auto p-4 text-sm text-muted-foreground">文件中没有数据行</div>
            ) : raw ? (
                <div className="sheet-source">
                    <pre>{slice.raw.join("\n")}</pre>
                    <div>
                        <Button variant="ghost" size="sm" disabled={!page} onClick={() => setPage((p) => p - 1)}>
                            上一页
                        </Button>
                        {page + 1} / {Math.max(1, Math.ceil(progress.rows / 100))}
                        <Button
                            variant="ghost"
                            size="sm"
                            disabled={(page + 1) * 100 >= progress.rows}
                            onClick={() => setPage((p) => p + 1)}
                        >
                            下一页
                        </Button>
                    </div>
                </div>
            ) : (
                <>
                    <div className="sheet-value">
                        <b>{address}</b>
                        <span title={value}>{value}</span>
                    </div>
                    <div
                        className="sheet-grid"
                        role="grid"
                        aria-label={format + " 表格"}
                        aria-rowcount={progress.rows}
                        aria-colcount={progress.columns}
                    >
                        <div className="sheet-corner" />
                        <div className="sheet-letters">
                            {cols.map((c) => (
                                <span
                                    key={c}
                                    style={{ left: c * COL - logicalX, width: COL }}
                                    data-active={c >= bounds.left && c <= bounds.right}
                                >
                                    {columnLabel(c)}
                                </span>
                            ))}
                        </div>
                        <div className="sheet-numbers">
                            {slice.rows.map((_, i) => (
                                <span
                                    key={i}
                                    style={{ top: (slice.start + i) * ROW - logicalY, height: ROW }}
                                    data-active={slice.start + i >= bounds.top && slice.start + i <= bounds.bottom}
                                >
                                    {slice.start + i + 1}
                                </span>
                            ))}
                        </div>
                        <div
                            ref={port}
                            className="sheet-port"
                            tabIndex={0}
                            aria-label="表格单元格"
                            onScroll={(e) => {
                                const n = e.currentTarget;
                                setViewport((v) => ({ ...v, x: n.scrollLeft, y: n.scrollTop }));
                            }}
                        >
                            <div style={{ width: horizontal.extent, height: vertical.extent, position: "relative" }}>
                                {slice.rows.map((values, i) => {
                                    const row = slice.start + i;
                                    return (
                                        <div
                                            role="row"
                                            key={row}
                                            className="sheet-row"
                                            style={{
                                                top: viewport.y + row * ROW - logicalY,
                                                left: viewport.x + firstCol * COL - logicalX,
                                                height: ROW,
                                            }}
                                        >
                                            {cols.map((col) => (
                                                <div
                                                    key={col}
                                                    role="gridcell"
                                                    aria-rowindex={row + 1}
                                                    aria-colindex={col + 1}
                                                    aria-selected={
                                                        row >= bounds.top &&
                                                        row <= bounds.bottom &&
                                                        col >= bounds.left &&
                                                        col <= bounds.right
                                                    }
                                                    data-current={row === cell.row && col === cell.col}
                                                    style={{ width: COL }}
                                                    title={values[col] ?? ""}
                                                    onPointerDown={(e) => {
                                                        if (e.button !== 0) return;
                                                        e.preventDefault();
                                                        port.current?.focus({ preventScroll: true });
                                                        dragging.current = true;
                                                        select({ row, col }, e.shiftKey);
                                                    }}
                                                    onPointerEnter={() => {
                                                        if (dragging.current) select({ row, col }, true);
                                                    }}
                                                >
                                                    {values[col]}
                                                </div>
                                            ))}
                                        </div>
                                    );
                                })}
                            </div>
                        </div>
                    </div>
                </>
            )}
            <div className="sheet-footer">
                <button
                    aria-pressed={!raw}
                    onClick={() => {
                        if (raw) {
                            restoreCell.current = cell;
                            setRaw(false);
                        }
                    }}
                >
                    <Table2 />
                    {format}
                </button>
                <button
                    aria-pressed={raw}
                    onClick={() => {
                        setPage(Math.floor(cell.row / 100));
                        setRaw(true);
                    }}
                >
                    <Text />
                    原文
                </button>
                <span>
                    {progress.rows.toLocaleString()} 行 × {progress.columns.toLocaleString()} 列
                    {!progress.complete && !error ? " · 正在索引…" : ""}
                </span>
            </div>
        </div>
    );
}
