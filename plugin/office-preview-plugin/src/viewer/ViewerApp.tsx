import React, { useCallback, useEffect, useRef, useState } from "react";
import { DocxEditor, type DocxEditorRef } from "@betteroffice/docx-react";
import { XlsxEditor, type XlsxAsyncEditorApi } from "@betteroffice/xlsx-react";
import { PptxEditor, type PptxAsyncEditorApi } from "@betteroffice/pptx-react";
import { BUNDLED_FONTS, loadBundledFontBytes } from "@betteroffice/fonts";
import type { PptxFontFace } from "@betteroffice/pptx";

type OfficeFormat = "docx" | "xlsx" | "pptx";

interface LoadMessage {
  type: "aipp-office:load";
  requestId: number;
  format: OfficeFormat;
  fileName: string;
  bytes: ArrayBuffer;
  isMobile?: boolean;
  colorMode?: "light" | "dark";
}

interface UnloadMessage {
  type: "aipp-office:unload";
}

type IncomingMessage = LoadMessage | UnloadMessage;

interface ActiveDoc {
  requestId: number;
  format: OfficeFormat;
  fileName: string;
  bytes: Uint8Array;
  isMobile: boolean;
  colorMode: "light" | "dark";
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
      loaded.push({
        family: face.family,
        bytes: new Uint8Array(buf),
        bold: false,
        italic: false,
      });
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

function postToHost(payload: Record<string, unknown>) {
  if (window.parent && window.parent !== window) {
    window.parent.postMessage(payload, "*");
  }
}

export default function ViewerApp() {
  const [activeDoc, setActiveDoc] = useState<ActiveDoc | null>(null);
  const [docxPages, setDocxPages] = useState<{ current: number; total: number }>({ current: 1, total: 0 });
  const [docxZoom, setDocxZoom] = useState<number>(1);
  const [pptxSlide, setPptxSlide] = useState<{ current: number; total: number }>({ current: 1, total: 0 });
  const activeRequestIdRef = useRef<number>(0);
  const docxRef = useRef<DocxEditorRef | null>(null);
  const pptxApiRef = useRef<PptxAsyncEditorApi | null>(null);
  const xlsxApiRef = useRef<XlsxAsyncEditorApi | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);

  const computeDocxInitialZoom = useCallback((isMobile: boolean) => {
    const width = containerRef.current?.clientWidth || window.innerWidth || 1024;
    if (!isMobile && width >= 860) return 1;
    const fitted = Math.floor(((width - 28) / 794) * 100) / 100;
    return Math.max(0.45, Math.min(1, fitted));
  }, []);

  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      // Only the embedding AIPP window may drive the viewer.
      if (event.source !== window.parent) return;
      const data = event.data as IncomingMessage | null;
      if (!data || typeof data !== "object" || typeof data.type !== "string") return;

      if (data.type === "aipp-office:unload") {
        activeRequestIdRef.current = 0;
        docxRef.current = null;
        pptxApiRef.current = null;
        xlsxApiRef.current = null;
        setActiveDoc(null);
        setDocxPages({ current: 1, total: 0 });
        setPptxSlide({ current: 1, total: 0 });
        return;
      }

      if (data.type === "aipp-office:load") {
        const reqId = data.requestId;
        activeRequestIdRef.current = reqId;
        docxRef.current = null;
        pptxApiRef.current = null;
        xlsxApiRef.current = null;

        if (!(data.bytes instanceof ArrayBuffer) || data.bytes.byteLength === 0) {
          postToHost({
            type: "aipp-office:error",
            requestId: reqId,
            code: "corrupt_file",
            message: "文件内容为空，无法预览",
          });
          return;
        }

        const isMobile = Boolean(data.isMobile);
        const initialZoom = data.format === "docx" ? computeDocxInitialZoom(isMobile) : 1;
        setDocxZoom(initialZoom);
        setDocxPages({ current: 1, total: 0 });
        setPptxSlide({ current: 1, total: 0 });

        const nextDoc: ActiveDoc = {
          requestId: reqId,
          format: data.format,
          fileName: data.fileName,
          bytes: new Uint8Array(data.bytes),
          isMobile,
          colorMode: data.colorMode === "dark" ? "dark" : "light",
        };
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
    postToHost({ type: "aipp-office:viewer-ready" });
    return () => {
      window.removeEventListener("message", onMessage);
    };
  }, [computeDocxInitialZoom]);

  // Wait for DOCX layout completion and notify host (discarding stale requestIds).
  useEffect(() => {
    if (!activeDoc || activeDoc.format !== "docx") return;
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
        postToHost({
          type: "aipp-office:error",
          requestId: reqId,
          code: "render_error",
          message: "DOCX 渲染器初始化超时",
        });
        return;
      }
      try {
        const total = await ref.whenLayoutComplete({ timeoutMs: 45000 });
        if (cancelled || activeRequestIdRef.current !== reqId) return;
        setDocxPages({ current: ref.getCurrentPage() || 1, total });
        postToHost({
          type: "aipp-office:loaded",
          requestId: reqId,
          format: "docx",
          meta: { pages: total },
        });
      } catch (err) {
        if (cancelled || activeRequestIdRef.current !== reqId) return;
        const classified = classifyRenderError(err);
        postToHost({ type: "aipp-office:error", requestId: reqId, ...classified });
      }
    };

    void waitDocx();
    return () => {
      cancelled = true;
    };
  }, [activeDoc]);

  const handleDocxZoom = (delta: number) => {
    const ref = docxRef.current;
    if (!ref) return;
    const next = Math.max(0.4, Math.min(2.5, Math.round((docxZoom + delta) * 100) / 100));
    ref.setZoom(next);
    setDocxZoom(next);
  };

  const handleDocxPageStep = (delta: number) => {
    const ref = docxRef.current;
    if (!ref || docxPages.total <= 0) return;
    const next = Math.max(1, Math.min(docxPages.total, docxPages.current + delta));
    ref.scrollToPage(next);
    setDocxPages((prev) => ({ ...prev, current: next }));
  };

  const handlePptxStep = async (delta: number) => {
    const api = pptxApiRef.current;
    if (!api || pptxSlide.total <= 0) return;
    const next = Math.max(1, Math.min(pptxSlide.total, pptxSlide.current + delta));
    if (await api.goToSlide(next)) {
      setPptxSlide((prev) => ({ ...prev, current: next }));
    }
  };

  return (
    <div
      ref={containerRef}
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
                disabled={docxPages.total <= 0 || docxPages.current >= docxPages.total}
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
          <div style={{ flex: 1, minHeight: 0, position: "relative" }}>
            <DocxEditor
              key={`docx-${activeDoc.requestId}`}
              ref={docxRef}
              documentBuffer={activeDoc.bytes}
              readOnly={true}
              showToolbar={false}
              showFileOpen={false}
              showHelpMenu={false}
              downloadOnSave={false}
              initialZoom={docxZoom}
              colorMode={activeDoc.colorMode}
              onError={(err) => {
                if (activeRequestIdRef.current !== activeDoc.requestId) return;
                const classified = classifyRenderError(err);
                postToHost({ type: "aipp-office:error", requestId: activeDoc.requestId, ...classified });
              }}
            />
          </div>
        </>
      )}

      {activeDoc?.format === "xlsx" && (
        <div style={{ flex: 1, minHeight: 0, position: "relative" }}>
          <XlsxEditor
            key={`xlsx-${activeDoc.requestId}`}
            file={activeDoc.bytes}
            readOnly={true}
            showToolbar={false}
            downloadOnSave={false}
            onReady={(api) => void (async () => {
              if (activeRequestIdRef.current !== activeDoc.requestId) return;
              xlsxApiRef.current = api;
              try {
                const catalog = await api.readCells({ ranges: [] });
                if (activeRequestIdRef.current !== activeDoc.requestId) return;
                postToHost({
                  type: "aipp-office:loaded",
                  requestId: activeDoc.requestId,
                  format: "xlsx",
                  meta: {
                    sheets: catalog.ok ? catalog.sheets.map((s) => s.name) : [],
                  },
                });
              } catch {
                if (activeRequestIdRef.current !== activeDoc.requestId) return;
                postToHost({
                  type: "aipp-office:loaded",
                  requestId: activeDoc.requestId,
                  format: "xlsx",
                  meta: {},
                });
              }
            })()}
            onError={(err) => {
              if (activeRequestIdRef.current !== activeDoc.requestId) return;
              const classified = classifyRenderError(err);
              postToHost({ type: "aipp-office:error", requestId: activeDoc.requestId, ...classified });
            }}
          />
        </div>
      )}

      {activeDoc?.format === "pptx" && (
        <>
          {pptxSlide.total > 1 && (
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
          <div style={{ flex: 1, minHeight: 0, position: "relative" }}>
            <PptxEditor
              key={`pptx-${activeDoc.requestId}`}
              file={activeDoc.bytes}
              readOnly={true}
              showToolbar={false}
              downloadOnSave={false}
              fonts={cachedPptxFallbackFonts ?? []}
              onReady={(api) => void (async () => {
                if (activeRequestIdRef.current !== activeDoc.requestId) return;
                pptxApiRef.current = api;
                if (activeRequestIdRef.current !== activeDoc.requestId) return;
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
                postToHost({
                  type: "aipp-office:loaded",
                  requestId: activeDoc.requestId,
                  format: "pptx",
                  meta: { slides: slideCount },
                });
              })()}
              onError={(err) => {
                if (activeRequestIdRef.current !== activeDoc.requestId) return;
                const classified = classifyRenderError(err);
                postToHost({ type: "aipp-office:error", requestId: activeDoc.requestId, ...classified });
              }}
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
