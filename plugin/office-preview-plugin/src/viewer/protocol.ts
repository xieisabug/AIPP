// Messages exchanged between the AIPP host window and this viewer iframe (postMessage).
// Keep in sync with src/utils/officeFile.ts and src/utils/officeEditProtocol.ts in the host.

export type OfficeFormat = "docx" | "xlsx" | "pptx";

/** Bumped when the host needs new viewer behaviour. 2 = selection, manual edit, edit jobs. */
export const VIEWER_PROTOCOL = 2;

export type ViewerMode = "view" | "edit" | "job";

export interface LoadMessage {
  type: "aipp-office:load";
  requestId: number;
  format: OfficeFormat;
  fileName: string;
  bytes: ArrayBuffer;
  isMobile?: boolean;
  colorMode?: "light" | "dark";
  /** Defaults to "view" (read-only). */
  mode?: "view" | "edit";
}

export interface UnloadMessage {
  type: "aipp-office:unload";
}

export interface SaveRequestMessage {
  type: "aipp-office:save-request";
  requestId: number;
}

/** AI-level edit steps; the viewer resolves them against the open document. */
export type OfficeEditStep =
  | { type: "replace_text"; find: string; replace: string; paragraph?: string; slide?: number }
  | { type: "replace_paragraph"; paragraph: string; text: string }
  | { type: "insert_paragraphs_after"; paragraph: string; paragraphs: string[] }
  | { type: "set_cells"; sheet: string; range: string; values: string[][] };

export interface EditJobMessage {
  type: "aipp-office:edit-job";
  requestId: number;
  format: OfficeFormat;
  fileName: string;
  bytes: ArrayBuffer;
  steps: OfficeEditStep[];
}

export type IncomingMessage = LoadMessage | UnloadMessage | SaveRequestMessage | EditJobMessage;

/** Stable description of what the user selected; never carries session-scoped ids. */
export type OfficeSelection =
  | {
      format: "docx";
      selectedText: string;
      paragraphText: string;
    }
  | {
      format: "xlsx";
      sheetName: string;
      sheetIndex: number;
      a1: string;
      /** Display text, row-major, clipped to MAX_SELECTION_ROWS x MAX_SELECTION_COLS. */
      cells: string[][];
      truncated: boolean;
    }
  | {
      format: "pptx";
      slide: number;
      kind: "slide" | "shape" | "text";
      /** Whole text of the selected shape (or of the slide when the slide itself is selected). */
      shapeText: string;
      selectedText: string;
    };

export interface EditJobFailure {
  code:
    | "missing_target"
    | "ambiguous_target"
    | "invalid_step"
    | "unsupported_edit"
    | "content_mismatch"
    | "locked_target"
    | "stale_version"
    | "render_error"
    | "save_failed";
  message: string;
  stepIndex?: number;
}

export const MAX_SELECTION_ROWS = 50;
export const MAX_SELECTION_COLS = 20;
export const MAX_SELECTION_TEXT = 4000;
