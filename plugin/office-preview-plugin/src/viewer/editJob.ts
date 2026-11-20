// Edit-job resolution: turns AI-level steps (stable text / sheet-name / slide-number anchors)
// into BetterOffice edit batches against the open document, applied with `expectVersion`.
// Every ambiguity is refused with a precise message instead of guessing a location.
import type { DocxPluginContext } from "@betteroffice/docx-react";
import type { XlsxPluginContext } from "@betteroffice/xlsx-react";
import type { PptxPluginContext } from "@betteroffice/pptx-react";
import { parseRangeA1, rangeA1 } from "./a1";
import type { EditJobFailure, OfficeEditStep } from "./protocol";

type DocxEditRequest = Parameters<NonNullable<DocxPluginContext<unknown>["edits"]>["applyEdits"]>[0];
type DocxStep = DocxEditRequest["steps"][number];
type XlsxEditRequest = Parameters<NonNullable<XlsxPluginContext<unknown>["edits"]>["applyEdits"]>[0];
type XlsxStep = XlsxEditRequest["steps"][number];
type PptxEditRequest = Parameters<NonNullable<PptxPluginContext<unknown>["edits"]>["applyEdits"]>[0];
type PptxStep = PptxEditRequest["steps"][number];

export type JobOutcome = { ok: true; changedSteps: number } | { ok: false; failure: EditJobFailure };

const MAX_JOB_CELLS = 10_000;

function fail(code: EditJobFailure["code"], message: string, stepIndex?: number): JobOutcome {
  return { ok: false, failure: { code, message, stepIndex } };
}

function preview(text: string, max = 60): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max)}…` : flat;
}

function countOccurrences(haystack: string, needle: string): number {
  if (!needle) return 0;
  let count = 0;
  let from = 0;
  for (;;) {
    const at = haystack.indexOf(needle, from);
    if (at < 0) return count;
    count += 1;
    from = at + needle.length;
  }
}

/** Maps a BetterOffice refusal (`{ ok: false, failure }`) to the job failure the host reports. */
export function mapRefusal(refusal: unknown, fallback = "文档拒绝了这次修改"): EditJobFailure {
  const failure = (refusal as { failure?: { code?: string; message?: string; stepIndex?: number } } | null)?.failure;
  const raw = failure?.code ?? "";
  const code: EditJobFailure["code"] =
    raw === "stale-version"
      ? "stale_version"
      : raw === "missing-target"
        ? "missing_target"
        : raw === "ambiguous-target"
          ? "ambiguous_target"
          : raw === "content-mismatch"
            ? "content_mismatch"
            : raw === "locked-target"
              ? "locked_target"
              : raw === "unsupported"
                ? "unsupported_edit"
                : "invalid_step";
  const detail = failure?.message ? `${failure.message}` : fallback;
  return { code, message: raw ? `${raw}：${detail}` : detail, stepIndex: failure?.stepIndex };
}

function isRefusal(value: unknown): value is { ok: false } {
  return Boolean(value) && (value as { ok?: unknown }).ok === false;
}

// ---------------------------------------------------------------- DOCX

export interface DocxParagraphRef {
  story: string;
  paraId: string;
  text: string;
}

/** Paragraphs whose text equals `anchor` (exact first, then ignoring surrounding whitespace). */
export function matchParagraphs(paragraphs: DocxParagraphRef[], anchor: string): DocxParagraphRef[] {
  const exact = paragraphs.filter((p) => p.text === anchor);
  if (exact.length > 0) return exact;
  const trimmed = anchor.trim();
  return paragraphs.filter((p) => p.text.trim() === trimmed);
}

export function resolveDocxStep(
  step: OfficeEditStep,
  index: number,
  paragraphs: DocxParagraphRef[],
): { ok: true; step: DocxStep } | { ok: false; failure: EditJobFailure } {
  const bad = (code: EditJobFailure["code"], message: string) => ({
    ok: false as const,
    failure: { code, message: `第 ${index + 1} 步：${message}`, stepIndex: index },
  });
  switch (step.type) {
    case "replace_text": {
      if (!step.find) return bad("invalid_step", "find 不能为空");
      if (step.find.includes("\n")) return bad("invalid_step", "find 只能在一个段落内，不能包含换行；跨段修改请逐段使用 replace_paragraph");
      if (step.replace.includes("\n")) return bad("invalid_step", "replace 不能包含换行；新增段落请使用 insert_paragraphs_after");
      let scope = paragraphs;
      if (step.paragraph != null && step.paragraph !== "") {
        scope = matchParagraphs(paragraphs, step.paragraph);
        if (scope.length === 0) {
          // 模型给的往往是段落的一部分：退而求其次按包含查找
          scope = paragraphs.filter((p) => p.text.includes(step.paragraph as string));
        }
        if (scope.length === 0) return bad("missing_target", `找不到所在段落「${preview(step.paragraph)}」，文档可能已变化`);
      }
      const hits = scope
        .map((p) => ({ p, n: countOccurrences(p.text, step.find) }))
        .filter((h) => h.n > 0);
      const total = hits.reduce((sum, h) => sum + h.n, 0);
      if (total === 0) return bad("missing_target", `找不到要替换的文字「${preview(step.find)}」`);
      if (total > 1) {
        return bad(
          "ambiguous_target",
          `「${preview(step.find)}」出现 ${total} 次，无法确定改哪一处；请在 find 中带上更多上下文，或用 paragraph 指明所在段落全文`,
        );
      }
      const target = hits[0].p;
      return {
        ok: true,
        step: {
          op: "replaceText",
          target: {
            kind: "search",
            text: step.find,
            within: { kind: "paragraph", story: target.story, paraId: target.paraId },
            view: "accepted",
          },
          text: step.replace,
        },
      };
    }
    case "replace_paragraph": {
      if (!step.paragraph) return bad("invalid_step", "paragraph 需要给出当前段落的完整原文");
      if (step.text.includes("\n")) return bad("invalid_step", "text 不能包含换行；新增段落请使用 insert_paragraphs_after");
      const found = matchParagraphs(paragraphs, step.paragraph);
      if (found.length === 0) return bad("missing_target", `找不到原文完全一致的段落「${preview(step.paragraph)}」，文档可能已变化`);
      if (found.length > 1) return bad("ambiguous_target", `有 ${found.length} 个段落原文相同「${preview(step.paragraph)}」，请改用 replace_text 并带上下文`);
      const target = found[0];
      return {
        ok: true,
        step: {
          op: "replaceText",
          target: { kind: "paragraph", story: target.story, paraId: target.paraId },
          text: step.text,
          expect: { text: target.text },
        },
      };
    }
    case "insert_paragraphs_after": {
      if (!step.paragraph) return bad("invalid_step", "paragraph 需要给出插入位置之前那个段落的完整原文");
      const texts = Array.isArray(step.paragraphs) ? step.paragraphs : [];
      if (texts.length === 0) return bad("invalid_step", "paragraphs 至少包含一个新段落");
      if (texts.some((t) => typeof t !== "string" || t.includes("\n"))) return bad("invalid_step", "每个新段落是一个不含换行的字符串");
      const found = matchParagraphs(paragraphs, step.paragraph);
      if (found.length === 0) return bad("missing_target", `找不到原文完全一致的段落「${preview(step.paragraph)}」`);
      if (found.length > 1) return bad("ambiguous_target", `有 ${found.length} 个段落原文相同「${preview(step.paragraph)}」，无法确定插入位置`);
      const target = found[0];
      return {
        ok: true,
        step: {
          op: "insertParagraphs",
          target: { story: target.story, paraId: target.paraId },
          at: "end",
          paragraphs: texts.map((text) => ({ text })),
        },
      };
    }
    default:
      return bad("unsupported_edit", `Word 文档不支持 ${(step as { type?: string }).type ?? "未知"} 类型的修改（支持 replace_text / replace_paragraph / insert_paragraphs_after）`);
  }
}

export async function runDocxJob(ctx: DocxPluginContext<unknown>, steps: OfficeEditStep[]): Promise<JobOutcome> {
  const read = await ctx.read.readStories({ stories: "all", view: "accepted" });
  if (isRefusal(read)) return { ok: false, failure: mapRefusal(read, "读取文档段落失败") };
  const paragraphs: DocxParagraphRef[] = [];
  for (const story of read.stories) {
    if (!story.ok) continue;
    for (const p of story.paragraphs) paragraphs.push({ story: story.story, paraId: p.paraId, text: p.text });
  }
  const batch: DocxStep[] = [];
  for (let i = 0; i < steps.length; i += 1) {
    const resolved = resolveDocxStep(steps[i], i, paragraphs);
    if (!resolved.ok) return resolved;
    batch.push(resolved.step);
  }
  if (!ctx.edits) return fail("render_error", "编辑器没有授予修改权限（插件未获写入授权）");
  const result = await ctx.edits.applyEdits({ expectVersion: read.version, source: "agent", steps: batch });
  if (isRefusal(result)) return { ok: false, failure: mapRefusal(result) };
  const changed = result.receipts.filter((r) => r.changed).length;
  if (!result.applied || changed === 0) return fail("invalid_step", "所有修改都没有改变文档（替换内容与原文相同）");
  return { ok: true, changedSteps: changed };
}

// ---------------------------------------------------------------- XLSX

export async function runXlsxJob(ctx: XlsxPluginContext<unknown>, steps: OfficeEditStep[]): Promise<JobOutcome> {
  const catalog = await ctx.read.readCells({ ranges: [] });
  if (isRefusal(catalog)) return { ok: false, failure: mapRefusal(catalog, "读取工作表目录失败") };
  const names = catalog.sheets.map((s) => s.name);
  const batch: XlsxStep[] = [];
  let totalCells = 0;
  for (let i = 0; i < steps.length; i += 1) {
    const step = steps[i];
    const prefix = `第 ${i + 1} 步：`;
    if (step.type !== "set_cells") {
      return fail("unsupported_edit", `${prefix}Excel 表格不支持 ${step.type} 类型的修改（支持 set_cells）`, i);
    }
    let sheet = catalog.sheets.filter((s) => s.name === step.sheet);
    if (sheet.length === 0) sheet = catalog.sheets.filter((s) => s.name.trim().toLowerCase() === step.sheet.trim().toLowerCase());
    if (sheet.length !== 1) {
      return fail("missing_target", `${prefix}找不到工作表「${step.sheet}」，现有工作表：${names.join("、") || "（无）"}`, i);
    }
    if (!sheet[0].editable) return fail("locked_target", `${prefix}工作表「${sheet[0].name}」受保护或不是普通工作表，不能写入`, i);
    const range = parseRangeA1(step.range);
    if (!range) return fail("invalid_step", `${prefix}区域「${step.range}」不是有效的 A1 地址（例如 B2 或 B2:D5）`, i);
    const rows = range.bottom - range.top + 1;
    const cols = range.right - range.left + 1;
    const values = Array.isArray(step.values) ? step.values : [];
    if (values.length !== rows || values.some((row) => !Array.isArray(row) || row.length !== cols)) {
      return fail("invalid_step", `${prefix}values 必须是 ${rows} 行 × ${cols} 列的二维数组，与区域 ${step.range} 一致`, i);
    }
    totalCells += rows * cols;
    if (totalCells > MAX_JOB_CELLS) return fail("invalid_step", `一次最多修改 ${MAX_JOB_CELLS} 个单元格`, i);
    batch.push({
      op: "setCellInputs",
      target: {
        sheetId: sheet[0].sheetId,
        range: { kind: "a1", a1: rangeA1(range.top, range.left, range.bottom, range.right) },
      },
      inputs: values.map((row) => row.map((v) => (v == null ? "" : String(v)))),
    });
  }
  if (!ctx.edits) return fail("render_error", "编辑器没有授予修改权限（插件未获写入授权）");
  const result = await ctx.edits.applyEdits({ expectVersion: catalog.version, source: "agent", steps: batch });
  if (isRefusal(result)) return { ok: false, failure: mapRefusal(result) };
  const changed = result.receipts.filter((r) => r.changed).length;
  if (!result.applied || changed === 0) return fail("invalid_step", "所有修改都没有改变表格（写入的值与原值相同）");
  return { ok: true, changedSteps: changed };
}

// ---------------------------------------------------------------- PPTX

export async function runPptxJob(ctx: PptxPluginContext<unknown>, steps: OfficeEditStep[]): Promise<JobOutcome> {
  const content = await ctx.read.readContent({});
  if (isRefusal(content)) return { ok: false, failure: mapRefusal(content, "读取幻灯片内容失败") };
  let version = content.version;
  const batch: PptxStep[] = [];
  for (let i = 0; i < steps.length; i += 1) {
    const step = steps[i];
    const prefix = `第 ${i + 1} 步：`;
    if (step.type !== "replace_text") {
      return fail("unsupported_edit", `${prefix}PowerPoint 不支持 ${step.type} 类型的修改（支持 replace_text）`, i);
    }
    if (!step.find) return fail("invalid_step", `${prefix}find 不能为空`, i);
    if (step.find.includes("\n") || step.replace.includes("\n")) {
      return fail("invalid_step", `${prefix}find / replace 只能在一个段落内，不能包含换行`, i);
    }
    let slideId: string | undefined;
    if (step.slide != null) {
      const slide = content.slides[step.slide - 1];
      if (!slide) return fail("missing_target", `${prefix}演示文稿只有 ${content.slides.length} 张幻灯片，没有第 ${step.slide} 张`, i);
      slideId = slide.id;
    }
    const found = await ctx.read.findText({ text: step.find, within: slideId ? { slideId } : undefined, limit: 50 });
    if (isRefusal(found)) return { ok: false, failure: mapRefusal(found, "查找文字失败") };
    version = found.version;
    let matches = found.matches;
    if (matches.length > 1 && step.paragraph) {
      const hint = step.paragraph;
      matches = matches.filter((m) =>
        content.stories.some((s) => s.shapeId === m.range.shapeId && s.storyId === m.range.storyId && s.text.includes(hint)),
      );
    }
    const where = step.slide != null ? `第 ${step.slide} 张幻灯片中` : "演示文稿中";
    if (matches.length === 0) return fail("missing_target", `${prefix}${where}找不到「${preview(step.find)}」`, i);
    if (matches.length > 1) {
      return fail("ambiguous_target", `${prefix}${where}「${preview(step.find)}」出现 ${matches.length} 次；请指定 slide，或在 find 中带上更多上下文`, i);
    }
    const r = matches[0].range;
    batch.push({
      op: "replaceText",
      target: { kind: "range", slideId: r.slideId, shapeId: r.shapeId, storyId: r.storyId, start: r.start, end: r.end },
      text: step.replace,
      expect: { text: step.find },
    });
  }
  if (!ctx.edits) return fail("render_error", "编辑器没有授予修改权限（插件未获写入授权）");
  const result = await ctx.edits.applyEdits({ expectVersion: version, source: "agent", steps: batch });
  if (isRefusal(result)) return { ok: false, failure: mapRefusal(result) };
  const changed = result.receipts.filter((r) => r.changed).length;
  if (!result.applied || changed === 0) return fail("invalid_step", "所有修改都没有改变演示文稿（替换内容与原文相同）");
  return { ok: true, changedSteps: changed };
}
