import React, { useCallback, useEffect, useRef, useState } from "react";
import { DocxEditor, defineDocxPlugin, type DocxEditorRef, type DocxPluginContext } from "@betteroffice/docx-react";
import {
  XlsxEditor,
  defineXlsxPlugin,
  type XlsxAsyncEditorApi,
  type XlsxPluginContext,
} from "@betteroffice/xlsx-react";
import {
  PptxEditor,
  definePptxPlugin,
  type PptxAsyncEditorApi,
  type PptxPluginContext,
} from "@betteroffice/pptx-react";
import { BUNDLED_FONTS, loadBundledFontBytes } from "@betteroffice/fonts";
import type { PptxFontFace } from "@betteroffice/pptx";
import { rangeA1 } from "./a1";
import { mapRefusal, runDocxJob, runPptxJob, runXlsxJob, type JobOutcome } from "./editJob";
import {
  MAX_SELECTION_COLS,
  MAX_SELECTION_ROWS,
  MAX_SELECTION_TEXT,
  VIEWER_PROTOCOL,
  type EditJobFailure,
  type IncomingMessage,
  type OfficeEditStep,
  type OfficeFormat,
  type OfficeSelection,
  type ViewerMode,
} from "./protocol";

interface ActiveDoc {
  requestId: number;
  format: OfficeFormat;
  fileName: string;
  bytes: Uint8Array;
  isMobile: boolean;
  colorMode: "light" | "dark";
  mode: ViewerMode;
  /** Present only in job mode. */
  steps?: OfficeEditStep[];
}

let cachedPptxFallbackFonts: PptxFontFace[] | null = null;

async function getPptxFallbackFonts(): Promise<PptxFontFace[]> {
  if (cachedPptxFallbackFonts) return cachedPptxFallbackFonts;
  const targets = BUNDLED_FONTS.filter(
    (face) =>
      (face.family === "Noto Sans SC" || face.family === "Noto Serif SC") &&
      face.weight === 400 &&
      face.style === "normal",
  );
  const loaded: PptxFontFace[] = [];
  for (const face of targets) {
    try {
      const buf = await loadBundledFontBytes(face);
      loaded.push({ family: face.family, bytes: new Uint8Array(buf), bold: false, italic: false });
    } catch {
      // Ignore individual optional face failure; fallback continues with available faces.
    }
  }
  cachedPptxFallbackFonts = loaded;
  return loaded;
}

function classifyRenderError(err: unknown): { code: string; message: string } {
  const raw = err instanceof Error ? err.message : String(err || "未知渲染错误");
  const lower = raw.toLowerCase();
  if (lower.includes("encrypt") || lower.includes("password")) {
    return { code: "password_encrypted", message: `文件受密码保护或已加密：${raw}` };
  }
  if (lower.includes("bad zip") || lower.includes("invalid zip") || lower.includes("eocd") || lower.includes("corrupt")) {
    return { code: "corrupt_file", message: `文件已损坏或结构异常：${raw}` };
  }
  return { code: "render_error", message: raw };
}

function postToHost(payload: Record<string, unknown>, transfer?: Transferable[]) {
  if (window.parent && window.parent !== window) {
    window.parent.postMessage(payload, "*", transfer);
  }
}

function clip(text: string, max = MAX_SELECTION_TEXT): string {
  return text.length > max ? `${text.slice(0, max)}…` : text;
}

const BRIDGE_PLUGIN_ID = "aipp-bridge";
const WRITE_GRANTS = { [BRIDGE_PLUGIN_ID]: { document: "write" as const, editBatches: true as const } };

/**
 * One document is open per iframe, so the editor plugins report to module-level hooks that the
 * React tree swaps per document. Plugins are defined once so the editors never restart them.
 */
type PluginEventLike = { type: string; selection?: unknown; layout?: unknown };
const bridge: {
  docx: ((ctx: DocxPluginContext<null>, event: PluginEventLike | null) => void) | null;
  xlsx: ((ctx: XlsxPluginContext<null>, event: PluginEventLike | null) => void) | null;
  pptx: ((ctx: PptxPluginContext<null>, event: PluginEventLike | null) => void) | null;
} = { docx: null, xlsx: null, pptx: null };

const docxBridgePlugin = defineDocxPlugin<null>({
  id: BRIDGE_PLUGIN_ID,
  createState: () => null,
  initialize: (ctx) => bridge.docx?.(ctx, null),
  onEvent: (ctx, event) => bridge.docx?.(ctx, event),
});
const xlsxBridgePlugin = defineXlsxPlugin<null>({
  id: BRIDGE_PLUGIN_ID,
  createState: () => null,
  initialize: (ctx) => bridge.xlsx?.(ctx, null),
  onEvent: (ctx, event) => bridge.xlsx?.(ctx, event),
});
const pptxBridgePlugin = definePptxPlugin<null>({
  id: BRIDGE_PLUGIN_ID,
  createState: () => null,
  initialize: (ctx) => bridge.pptx?.(ctx, null),
  onEvent: (ctx, event) => bridge.pptx?.(ctx, event),
});
const DOCX_PLUGINS = [docxBridgePlugin];
const XLSX_PLUGINS = [xlsxBridgePlugin];
const PPTX_PLUGINS = [pptxBridgePlugin];

/**
 * Resolves once `get()` returns a value, polling; rejects after `timeoutMs`. The value is boxed:
 * editor refs expose a throwing `then` getter in worker mode, so they must never be awaited.
 */
async function waitFor<T>(get: () => T | null | undefined, timeoutMs: number, what: string): Promise<{ value: T }> {
  const start = performance.now();
  for (;;) {
    const value = get();
    if (value != null) return { value };
    if (performance.now() - start > timeoutMs) throw new Error(`${what}超时`);
    await new Promise((r) => setTimeout(r, 50));
  }
}

/** Runs `action` with a fresh plugin context (the stored one may have been superseded). */
async function withFreshContext<C extends { run(action: (fresh: C) => unknown): Promise<void> }>(
  ctx: C,
  action: (fresh: C) => Promise<JobOutcome>,
): Promise<JobOutcome> {
  let outcome: JobOutcome | null = null;
  let error: unknown = null;
  await ctx.run(async (fresh) => {
    try {
      outcome = await action(fresh);
    } catch (err) {
      error = err;
    }
  });
  if (error) throw error;
  if (!outcome) throw new Error("编辑器插件没有执行修改（插件上下文已结束）");
  return outcome;
}

export default function ViewerApp() {
  const [activeDoc, setActiveDoc] = useState<ActiveDoc | null>(null);
  const [docxPages, setDocxPages] = useState<{ current: number; total: number }>({ current: 1, total: 0 });
  const [docxZoom, setDocxZoom] = useState<number>(1);
  const [pptxSlide, setPptxSlide] = useState<{ current: number; total: number }>({ current: 1, total: 0 });
  const activeRequestIdRef = useRef<number>(0);
  const activeDocRef = useRef<ActiveDoc | null>(null);
  /** requestId already reported to the host as loaded (report once per document). */
  const loadedReportedRef = useRef<number>(0);
  const dirtyReportedRef = useRef<number>(0);
  const docxRef = useRef<DocxEditorRef | null>(null);
  const pptxApiRef = useRef<PptxAsyncEditorApi | null>(null);
  const xlsxApiRef = useRef<XlsxAsyncEditorApi | null>(null);
  const docxCtxRef = useRef<DocxPluginContext<null> | null>(null);
  const xlsxCtxRef = useRef<XlsxPluginContext<null> | null>(null);
  const pptxCtxRef = useRef<PptxPluginContext<null> | null>(null);
  const selectionSeqRef = useRef(0);
  const selectionTimerRef = useRef<number | null>(null);
  const selectionRead = useRef<(() => Promise<OfficeSelection | null>) | null>(null);
  const dragStart = useRef<{ id: number; x: number; y: number } | null>(null);
  const dropPoint = useRef<{ x: number; y: number } | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);

  const resetEditors = useCallback(() => {
    docxRef.current = null;
    pptxApiRef.current = null;
    xlsxApiRef.current = null;
    docxCtxRef.current = null;
    xlsxCtxRef.current = null;
    pptxCtxRef.current = null;
    if (selectionTimerRef.current != null) window.clearTimeout(selectionTimerRef.current);
    selectionTimerRef.current = null;
    selectionRead.current = null;
    dragStart.current = null;
    dropPoint.current = null;
  }, []);

  const reportLoaded = useCallback((reqId: number, format: OfficeFormat, meta: Record<string, unknown>) => {
    if (activeRequestIdRef.current !== reqId || loadedReportedRef.current === reqId) return;
    loadedReportedRef.current = reqId;
    // Job documents report through edit-result only.
    if (activeDocRef.current?.mode === "job") return;
    postToHost({ type: "aipp-office:loaded", requestId: reqId, format, meta });
  }, []);

  const reportError = useCallback((reqId: number, err: unknown) => {
    if (activeRequestIdRef.current !== reqId) return;
    const classified = classifyRenderError(err);
    if (activeDocRef.current?.mode === "job") {
      postToHost({ type: "aipp-office:edit-result", requestId: reqId, ok: false, failure: { code: "render_error", message: classified.message } });
      return;
    }
    postToHost({ type: "aipp-office:error", requestId: reqId, ...classified });
  }, []);

  const reportDirty = useCallback((reqId: number) => {
    if (activeRequestIdRef.current !== reqId || dirtyReportedRef.current === reqId) return;
    if (activeDocRef.current?.mode !== "edit") return;
    dirtyReportedRef.current = reqId;
    postToHost({ type: "aipp-office:dirty", requestId: reqId });
  }, []);

  /** Debounced: selections settle while the user drags. */
  const scheduleSelection = useCallback((reqId: number, read: () => Promise<OfficeSelection | null>) => {
    if (activeDocRef.current?.mode !== "view") return;
    selectionRead.current = read;
    if (selectionTimerRef.current != null) window.clearTimeout(selectionTimerRef.current);
    const seq = ++selectionSeqRef.current;
    selectionTimerRef.current = window.setTimeout(() => {
      selectionTimerRef.current = null;
      void read()
        .catch((err) => {
          console.warn("[office-viewer] read selection failed:", err);
          return null;
        })
        .then((selection) => {
          if (seq !== selectionSeqRef.current || activeRequestIdRef.current !== reqId) return;
          const point = dropPoint.current;
          dropPoint.current = null;
          postToHost({ type: "aipp-office:selection", requestId: reqId, selection, point });
        });
    }, 220);
  }, []);

  const computeDocxInitialZoom = useCallback((isMobile: boolean) => {
    const width = containerRef.current?.clientWidth || window.innerWidth || 1024;
    if (!isMobile && width >= 860) return 1;
    const fitted = Math.floor(((width - 28) / 794) * 100) / 100;
    return Math.max(0.45, Math.min(1, fitted));
  }, []);

  const saveCurrent = useCallback(async (): Promise<ArrayBuffer> => {
    const doc = activeDocRef.current;
    if (!doc) throw new Error("没有打开的文档");
    if (doc.format === "docx") {
      const ref = (await waitFor(() => docxRef.current, 15000, "等待 Word 编辑器")).value;
      const buf = await ref.save();
      if (!buf) throw new Error("Word 编辑器没有返回文件内容");
      return buf;
    }
    if (doc.format === "xlsx") {
      const api = (await waitFor(() => xlsxApiRef.current, 15000, "等待 Excel 编辑器")).value;
      const bytes = await api.save();
      return bytes.slice().buffer as ArrayBuffer;
    }
    const api = (await waitFor(() => pptxApiRef.current, 15000, "等待 PowerPoint 编辑器")).value;
    const bytes = await api.save();
    return bytes.slice().buffer as ArrayBuffer;
  }, []);

  // ---- plugin bridges (selection reporting, job contexts)
  // Selection reads go through the editor APIs, not the event's plugin context: later events
  // (layout changes) supersede that context before the debounced read runs.
  useEffect(() => {
    const pptxSlideText = async (slideId: string): Promise<{ stories: { shapeId: string; storyId: string; text: string }[] } | null> => {
      const api = pptxApiRef.current;
      if (!api) return null;
      const access = await api.handleAsync();
      const content = await access.readContent({ slideIds: [slideId] });
      if (!content.ok) return null;
      return { stories: content.stories.filter((s) => s.slideId === slideId) };
    };

    bridge.docx = (ctx, event) => {
      docxCtxRef.current = ctx;
      if (event?.type !== "selection-change") return;
      const reqId = activeRequestIdRef.current;
      scheduleSelection(reqId, async () => {
        const info = await docxRef.current?.readSelectionInfo();
        const selected = info?.selectedText ?? "";
        if (!info || !selected.trim()) return null;
        const paragraph = info.paragraphText || "";
        let paragraphOccurrence: number | undefined;
        if (info.paraId && paragraph && paragraph.length <= MAX_SELECTION_TEXT) {
          const matches = await docxRef.current?.findParagraphs(paragraph, { caseSensitive: true, limit: 256 });
          const exact = matches?.filter(m => !m.before && !m.after && m.match === paragraph) ?? [];
          const index = exact.findIndex(m => m.paraId === info.paraId);
          if (exact.length > 1 && index >= 0) paragraphOccurrence = index + 1;
        }
        return { format: "docx", selectedText: clip(selected), paragraphText: clip(paragraph), paragraphOccurrence,
          before: info.before?.slice(-80), after: info.after?.slice(0, 80) };
      });
    };

    bridge.xlsx = (ctx, event) => {
      xlsxCtxRef.current = ctx;
      if (event?.type !== "selection-change") return;
      const reqId = activeRequestIdRef.current;
      const selection = event.selection as {
        sheetId: string;
        sheetIndex: number;
        cells: { anchor: { row: number; col: number }; focus: { row: number; col: number }; range?: { top: number; left: number; bottom: number; right: number } } | null;
      } | null;
      scheduleSelection(reqId, async () => {
        const api = xlsxApiRef.current;
        if (!selection?.cells || !api) return null;
        const { anchor, focus } = selection.cells;
        const box = selection.cells.range ?? {
          top: Math.min(anchor.row, focus.row),
          left: Math.min(anchor.col, focus.col),
          bottom: Math.max(anchor.row, focus.row),
          right: Math.max(anchor.col, focus.col),
        };
        const a1 = rangeA1(box.top, box.left, box.bottom, box.right);
        const clipBottom = Math.min(box.bottom, box.top + MAX_SELECTION_ROWS - 1);
        const clipRight = Math.min(box.right, box.left + MAX_SELECTION_COLS - 1);
        // Plugin sheet ids and editor-API sheet ids are separate sessions: map by position.
        const catalog = await api.readCells({ ranges: [] });
        if (!catalog.ok) return null;
        const sheet = catalog.sheets[selection.sheetIndex];
        if (!sheet) return null;
        const read = await api.readCells({
          ranges: [{ sheetId: sheet.sheetId, range: { kind: "a1", a1: rangeA1(box.top, box.left, clipBottom, clipRight) } }],
        });
        if (!read.ok) {
          console.warn("[office-viewer] read selected cells refused:", read.failure);
          return null;
        }
        const sheetName = sheet.name;
        const cells = (read.ranges[0]?.cells ?? []).map((row) => row.map((cell) => cell.displayText ?? ""));
        return {
          format: "xlsx",
          sheetName,
          sheetIndex: selection.sheetIndex,
          a1,
          cells,
          truncated: clipBottom < box.bottom || clipRight < box.right,
        };
      });
    };

    // The read-only deck has no shape selection, so the slide on screen is the selection; shape
    // and text selections (edit mode) narrow it down when they arrive.
    let lastSlideId: string | null = null;
    bridge.pptx = (ctx, event) => {
      pptxCtxRef.current = ctx;
      if (!event) {
        lastSlideId = null;
        return;
      }
      const reqId = activeRequestIdRef.current;
      if (event.type === "layout-change") {
        const layout = event.layout as { slideId: string; slide: number } | null;
        if (!layout || layout.slideId === lastSlideId) return;
        lastSlideId = layout.slideId;
        setPptxSlide((prev) => (prev.current === layout.slide ? prev : { ...prev, current: layout.slide }));
        scheduleSelection(reqId, async () => {
          const read = await pptxSlideText(layout.slideId);
          if (!read) return null;
          const text = read.stories.map((s) => s.text).filter(Boolean).join("\n");
          return { format: "pptx", slide: layout.slide, kind: "slide", shapeText: clip(text), selectedText: "" };
        });
        return;
      }
      if (event.type !== "selection-change") return;
      const selection = event.selection as {
        slideId: string;
        slide: number;
        target: { kind: "slide" } | { kind: "shape"; shapeId: string } | { kind: "text"; shapeId: string; storyId: string; anchor: number; focus: number };
      } | null;
      if (!selection) return;
      scheduleSelection(reqId, async () => {
        const read = await pptxSlideText(selection.slideId);
        if (!read) return null;
        const target = selection.target;
        if (target.kind === "slide") {
          const text = read.stories.map((s) => s.text).filter(Boolean).join("\n");
          return { format: "pptx", slide: selection.slide, kind: "slide", shapeText: clip(text), selectedText: "" };
        }
        const shapeStories = read.stories.filter((s) => s.shapeId === target.shapeId);
        const shapeText = shapeStories.map((s) => s.text).join("\n");
        let selectedText = "";
        if (target.kind === "text") {
          const story = shapeStories.find((s) => s.storyId === target.storyId);
          if (story) selectedText = story.text.slice(Math.min(target.anchor, target.focus), Math.max(target.anchor, target.focus));
        }
        return { format: "pptx", slide: selection.slide, kind: target.kind, shapeText: clip(shapeText), selectedText: clip(selectedText) };
      });
    };
    return () => {
      bridge.docx = null;
      bridge.xlsx = null;
      bridge.pptx = null;
    };
  }, [scheduleSelection]);

  // ---- host messages
  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      // Only the embedding AIPP window may drive the viewer.
      if (event.source !== window.parent) return;
      const data = event.data as IncomingMessage | null;
      if (!data || typeof data !== "object" || typeof data.type !== "string") return;

      if (data.type === "aipp-office:unload") {
        activeRequestIdRef.current = 0;
        activeDocRef.current = null;
        resetEditors();
        setActiveDoc(null);
        setDocxPages({ current: 1, total: 0 });
        setPptxSlide({ current: 1, total: 0 });
        return;
      }

      if (data.type === "aipp-office:save-request") {
        const reqId = data.requestId;
        if (reqId !== activeRequestIdRef.current) return;
        void saveCurrent()
          .then((buf) => postToHost({ type: "aipp-office:saved", requestId: reqId, bytes: buf }, [buf]))
          .catch((err) =>
            postToHost({ type: "aipp-office:save-error", requestId: reqId, message: err instanceof Error ? err.message : String(err) }),
          );
        return;
      }

      if (data.type === "aipp-office:load" || data.type === "aipp-office:edit-job") {
        const reqId = data.requestId;
        const isJob = data.type === "aipp-office:edit-job";
        activeRequestIdRef.current = reqId;
        resetEditors();

        if (!(data.bytes instanceof ArrayBuffer) || data.bytes.byteLength === 0) {
          if (isJob) {
            postToHost({ type: "aipp-office:edit-result", requestId: reqId, ok: false, failure: { code: "render_error", message: "文件内容为空" } });
          } else {
            postToHost({ type: "aipp-office:error", requestId: reqId, code: "corrupt_file", message: "文件内容为空，无法预览" });
          }
          return;
        }

        const isMobile = !isJob && Boolean((data as { isMobile?: boolean }).isMobile);
        const initialZoom = data.format === "docx" && !isJob ? computeDocxInitialZoom(isMobile) : 1;
        setDocxZoom(initialZoom);
        setDocxPages({ current: 1, total: 0 });
        setPptxSlide({ current: 1, total: 0 });

        const nextDoc: ActiveDoc = {
          requestId: reqId,
          format: data.format,
          fileName: data.fileName,
          bytes: new Uint8Array(data.bytes),
          isMobile,
          colorMode: !isJob && (data as { colorMode?: string }).colorMode === "dark" ? "dark" : "light",
          mode: isJob ? "job" : (data as { mode?: string }).mode === "edit" ? "edit" : "view",
          steps: isJob ? (data as { steps: OfficeEditStep[] }).steps : undefined,
        };
        activeDocRef.current = nextDoc;
        if (data.format !== "pptx") {
          setActiveDoc(nextDoc);
          return;
        }
        // PPTX needs its CJK-capable faces before the editor opens the deck.
        void getPptxFallbackFonts().then(() => {
          if (activeRequestIdRef.current === reqId) setActiveDoc(nextDoc);
        });
      }
    };

    window.addEventListener("message", onMessage);
    postToHost({ type: "aipp-office:viewer-ready", protocol: VIEWER_PROTOCOL });
    return () => {
      window.removeEventListener("message", onMessage);
    };
  }, [computeDocxInitialZoom, resetEditors, saveCurrent]);

  // ---- edit jobs: wait for the editor and its plugin context, apply, save, report once
  useEffect(() => {
    if (!activeDoc || activeDoc.mode !== "job") return;
    const reqId = activeDoc.requestId;
    const steps = activeDoc.steps ?? [];
    let cancelled = false;

    const run = async (): Promise<{ outcome: JobOutcome; bytes?: ArrayBuffer }> => {
      let outcome: JobOutcome;
      if (activeDoc.format === "docx") {
        const ctx = (await waitFor(() => docxCtxRef.current, 60000, "等待 Word 文档加载")).value;
        await waitFor(() => docxRef.current, 15000, "等待 Word 编辑器");
        outcome = await withFreshContext(ctx, (fresh) => runDocxJob(fresh as DocxPluginContext<unknown>, steps));
      } else if (activeDoc.format === "xlsx") {
        const api = (await waitFor(() => xlsxApiRef.current, 60000, "等待 Excel 表格加载")).value;
        await api.whenHydrated();
        const ctx = (await waitFor(() => xlsxCtxRef.current, 15000, "等待 Excel 插件上下文")).value;
        outcome = await withFreshContext(ctx, (fresh) => runXlsxJob(fresh as XlsxPluginContext<unknown>, steps));
      } else {
        const api = (await waitFor(() => pptxApiRef.current, 60000, "等待 PowerPoint 加载")).value;
        await api.whenHydrated();
        const ctx = (await waitFor(() => pptxCtxRef.current, 15000, "等待 PowerPoint 插件上下文")).value;
        outcome = await withFreshContext(ctx, (fresh) => runPptxJob(fresh as PptxPluginContext<unknown>, steps));
      }
      if (!outcome.ok) return { outcome };
      return { outcome, bytes: await saveCurrent() };
    };

    void run()
      .then(({ outcome, bytes }) => {
        if (cancelled || activeRequestIdRef.current !== reqId) return;
        if (!outcome.ok) {
          postToHost({ type: "aipp-office:edit-result", requestId: reqId, ok: false, failure: outcome.failure });
          return;
        }
        postToHost(
          { type: "aipp-office:edit-result", requestId: reqId, ok: true, changedSteps: outcome.changedSteps, bytes },
          bytes ? [bytes] : undefined,
        );
      })
      .catch((err) => {
        if (cancelled || activeRequestIdRef.current !== reqId) return;
        const message = err instanceof Error ? err.message : String(err);
        const failure: EditJobFailure = message.includes("save")
          ? { code: "save_failed", message }
          : (err as { failure?: unknown })?.failure
            ? mapRefusal(err)
            : { code: "render_error", message };
        postToHost({ type: "aipp-office:edit-result", requestId: reqId, ok: false, failure });
      });
    return () => {
      cancelled = true;
    };
  }, [activeDoc, saveCurrent]);

  // DOCX reports "loaded" as soon as its first page is painted (onFirstPagePainted).
  // Full layout keeps running in the background only to learn the page count: it waits for
  // layout the editor runs on its own and may take long or never settle for some documents,
  // so it must not gate the loading state or turn an already visible document into an error.
  useEffect(() => {
    if (!activeDoc || activeDoc.format !== "docx" || activeDoc.mode === "job") return;
    const reqId = activeDoc.requestId;
    let cancelled = false;

    const waitDocx = async () => {
      const start = performance.now();
      while (!docxRef.current && !cancelled && performance.now() - start < 15000) {
        await new Promise((r) => setTimeout(r, 40));
      }
      if (cancelled || activeRequestIdRef.current !== reqId) return;
      const ref = docxRef.current;
      if (!ref) {
        if (loadedReportedRef.current === reqId) return;
        postToHost({ type: "aipp-office:error", requestId: reqId, code: "render_error", message: "DOCX 渲染器初始化超时" });
        return;
      }
      try {
        const total = await ref.whenLayoutComplete({ timeoutMs: 45000 });
        if (cancelled || activeRequestIdRef.current !== reqId) return;
        setDocxPages({ current: ref.getCurrentPage() || 1, total });
        reportLoaded(reqId, "docx", { pages: total });
      } catch (err) {
        if (cancelled || activeRequestIdRef.current !== reqId) return;
        if (loadedReportedRef.current === reqId) {
          console.warn("[office-viewer] whenLayoutComplete did not settle:", err);
          return;
        }
        reportError(reqId, err);
      }
    };

    void waitDocx();
    return () => {
      cancelled = true;
    };
  }, [activeDoc, reportLoaded, reportError]);

  const handleDocxZoom = (delta: number) => {
    const ref = docxRef.current;
    if (!ref) return;
    const next = Math.max(0.4, Math.min(2.5, Math.round((docxZoom + delta) * 100) / 100));
    ref.setZoom(next);
    setDocxZoom(next);
  };

  const handleDocxPageStep = (delta: number) => {
    const ref = docxRef.current;
    const total = docxPages.total || ref?.getTotalPages() || 0;
    if (!ref || total <= 0) return;
    const next = Math.max(1, Math.min(total, docxPages.current + delta));
    ref.scrollToPage(next);
    setDocxPages({ current: next, total });
  };

  const handlePptxStep = async (delta: number) => {
    const api = pptxApiRef.current;
    if (!api || pptxSlide.total <= 0) return;
    const next = Math.max(1, Math.min(pptxSlide.total, pptxSlide.current + delta));
    if (await api.goToSlide(next)) {
      setPptxSlide((prev) => ({ ...prev, current: next }));
    }
  };

  /** Ctrl/Cmd+S inside an editor: the host owns saving (always a copy). */
  const onSaveShortcut = useCallback(() => {
    const doc = activeDocRef.current;
    if (doc?.mode === "edit") postToHost({ type: "aipp-office:save-shortcut", requestId: doc.requestId });
    return false;
  }, []);

  const readOnly = activeDoc?.mode !== "edit" && activeDoc?.mode !== "job";
  const showEditorToolbar = activeDoc?.mode === "edit";
  const grants = readOnly ? undefined : WRITE_GRANTS;

  return (
    <div
      ref={containerRef}
      onPointerDownCapture={(event) => {
        dropPoint.current = null;
        dragStart.current = null;
        if (!event.isPrimary || event.button !== 0 || !(event.target as Element).closest("[data-selection-surface]")) return;
        dragStart.current = { id: event.pointerId, x: event.clientX, y: event.clientY };
      }}
      onPointerCancelCapture={() => { dragStart.current = null; dropPoint.current = null; }}
      onDoubleClickCapture={(event) => {
        if (activeDoc?.format !== "docx" || !(event.target as Element).closest("[data-selection-surface]")) return;
        dropPoint.current = { x: event.clientX, y: event.clientY };
        if (selectionRead.current) scheduleSelection(activeRequestIdRef.current, selectionRead.current);
      }}
      onPointerUpCapture={(event) => {
        const start = dragStart.current;
        dragStart.current = null;
        if (!start || start.id !== event.pointerId || Math.hypot(event.clientX - start.x, event.clientY - start.y) < 3) return;
        dropPoint.current = { x: event.clientX, y: event.clientY };
        if (selectionRead.current) scheduleSelection(activeRequestIdRef.current, selectionRead.current);
      }}
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100vh",
        width: "100vw",
        overflow: "hidden",
        background: activeDoc?.colorMode === "dark" ? "#111827" : "#f8fafc",
        color: activeDoc?.colorMode === "dark" ? "#e5e7eb" : "#0f172a",
        fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
      }}
    >
      {activeDoc?.format === "docx" && (
        <>
          {activeDoc.mode === "view" && (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                gap: 8,
                padding: "6px 12px",
                borderBottom: "1px solid rgba(148,163,184,0.28)",
                fontSize: 12,
                flexShrink: 0,
              }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                <button type="button" onClick={() => handleDocxPageStep(-1)} disabled={docxPages.current <= 1} style={btnStyle}>
                  上一页
                </button>
                <span style={{ fontVariantNumeric: "tabular-nums" }}>
                  第 {docxPages.current} / {docxPages.total || "-"} 页
                </span>
                <button
                  type="button"
                  onClick={() => handleDocxPageStep(1)}
                  disabled={docxPages.total > 0 && docxPages.current >= docxPages.total}
                  style={btnStyle}
                >
                  下一页
                </button>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                <button type="button" onClick={() => handleDocxZoom(-0.15)} style={btnStyle}>
                  -
                </button>
                <span style={{ minWidth: 42, textAlign: "center", fontVariantNumeric: "tabular-nums" }}>
                  {Math.round(docxZoom * 100)}%
                </span>
                <button type="button" onClick={() => handleDocxZoom(0.15)} style={btnStyle}>
                  +
                </button>
              </div>
            </div>
          )}
          <div data-selection-surface style={{ flex: 1, minHeight: 0, position: "relative" }}>
            <DocxEditor
              key={`docx-${activeDoc.requestId}`}
              ref={docxRef}
              documentBuffer={activeDoc.bytes}
              readOnly={readOnly}
              showToolbar={showEditorToolbar}
              showFileOpen={false}
              showHelpMenu={false}
              downloadOnSave={false}
              onSaveRequest={onSaveShortcut}
              initialZoom={docxZoom}
              colorMode={activeDoc.colorMode}
              plugins={DOCX_PLUGINS}
              pluginGrants={grants}
              onDocumentChange={() => reportDirty(activeDoc.requestId)}
              onFirstPagePainted={() => {
                const pages = docxRef.current?.getTotalPages() || 0;
                if (pages > 0) setDocxPages((prev) => ({ ...prev, total: pages }));
                reportLoaded(activeDoc.requestId, "docx", { pages: pages || null, firstPaint: true });
              }}
              onError={(err) => reportError(activeDoc.requestId, err)}
            />
          </div>
        </>
      )}

      {activeDoc?.format === "xlsx" && (
        <div data-selection-surface style={{ flex: 1, minHeight: 0, position: "relative" }}>
          <XlsxEditor
            key={`xlsx-${activeDoc.requestId}`}
            file={activeDoc.bytes}
            fileName={activeDoc.fileName}
            readOnly={readOnly}
            showToolbar={showEditorToolbar}
            downloadOnSave={false}
            onSaveRequest={onSaveShortcut}
            plugins={XLSX_PLUGINS}
            pluginGrants={grants}
            onDocumentChange={() => reportDirty(activeDoc.requestId)}
            onReady={(api) => void (async () => {
              if (activeRequestIdRef.current !== activeDoc.requestId) return;
              xlsxApiRef.current = api;
              try {
                const catalog = await api.readCells({ ranges: [] });
                if (activeRequestIdRef.current !== activeDoc.requestId) return;
                reportLoaded(activeDoc.requestId, "xlsx", { sheets: catalog.ok ? catalog.sheets.map((s) => s.name) : [] });
              } catch {
                if (activeRequestIdRef.current !== activeDoc.requestId) return;
                reportLoaded(activeDoc.requestId, "xlsx", {});
              }
            })()}
            onError={(err) => reportError(activeDoc.requestId, err)}
          />
        </div>
      )}

      {activeDoc?.format === "pptx" && (
        <>
          {activeDoc.mode === "view" && pptxSlide.total > 1 && (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                gap: 8,
                padding: "6px 12px",
                borderBottom: "1px solid rgba(148,163,184,0.28)",
                fontSize: 12,
                flexShrink: 0,
              }}
            >
              <button type="button" onClick={() => void handlePptxStep(-1)} disabled={pptxSlide.current <= 1} style={btnStyle}>
                上一页
              </button>
              <span style={{ fontVariantNumeric: "tabular-nums" }}>
                幻灯片 {pptxSlide.current} / {pptxSlide.total}
              </span>
              <button
                type="button"
                onClick={() => void handlePptxStep(1)}
                disabled={pptxSlide.current >= pptxSlide.total}
                style={btnStyle}
              >
                下一页
              </button>
            </div>
          )}
          <div data-selection-surface style={{ flex: 1, minHeight: 0, position: "relative" }}>
            <PptxEditor
              key={`pptx-${activeDoc.requestId}`}
              file={activeDoc.bytes}
              readOnly={readOnly}
              showToolbar={showEditorToolbar}
              downloadOnSave={false}
              onSaveRequest={onSaveShortcut}
              fonts={cachedPptxFallbackFonts ?? []}
              plugins={PPTX_PLUGINS}
              pluginGrants={grants}
              onDocumentChange={() => reportDirty(activeDoc.requestId)}
              onReady={(api) => void (async () => {
                if (activeRequestIdRef.current !== activeDoc.requestId) return;
                pptxApiRef.current = api;
                let slideCount = 0;
                try {
                  const structured = await api.exportStructured();
                  if (structured.ok && Array.isArray(structured.content?.slides)) {
                    slideCount = structured.content.slides.length;
                  }
                } catch {
                  // Optional metadata read
                }
                if (activeRequestIdRef.current !== activeDoc.requestId) return;
                setPptxSlide({ current: 1, total: slideCount });
                reportLoaded(activeDoc.requestId, "pptx", { slides: slideCount });
              })()}
              onError={(err) => reportError(activeDoc.requestId, err)}
            />
          </div>
        </>
      )}
    </div>
  );
}

const btnStyle: React.CSSProperties = {
  border: "1px solid rgba(148,163,184,0.45)",
  background: "transparent",
  color: "inherit",
  borderRadius: 6,
  padding: "3px 9px",
  fontSize: 12,
  cursor: "pointer",
};
