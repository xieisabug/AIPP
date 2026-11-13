import type { AgentActivityEvent } from "@/data/Conversation";

/** Older Pi responses saved only tool names and JSON output. Adapt on read. */
export function normalizePiActivity(activity: AgentActivityEvent): AgentActivityEvent {
  if (activity.agent_kind !== "pi" || activity.kind !== "tool") return activity;
  const metadata = activity.metadata as Record<string, unknown> | null;
  if (metadata?.tool) return activity; // Current bridge already classifies operations.
  const tool = activity.title ?? "";
  const input = activity.input as Record<string, unknown> | null;
  const path = typeof input?.path === "string" ? input.path : "";
  const pattern = typeof input?.pattern === "string" ? input.pattern : "";
  let kind = activity.kind, title = activity.title, output = activity.output;
  let rawResult: unknown;
  try {
    rawResult = output ? JSON.parse(output) : undefined;
    if (rawResult === null) output = undefined;
    else if (rawResult && typeof rawResult === "object" && "content" in rawResult && Array.isArray(rawResult.content)) {
      output = rawResult.content.map((part: { type?: string; text?: string }) =>
        part.type === "text" ? part.text : part.type === "image" ? "[图片结果]" : `[${part.type || "非文本结果"}]`).join("\n");
    }
  } catch { /* Keep old non-JSON output verbatim. */ }
  const details = (rawResult as { details?: { patch?: string; diff?: string } } | null)?.details;
  const extra: Record<string, unknown> = { ...metadata, tool, rawResult };
  if (["bash", "powershell"].includes(tool)) {
    kind = "command";
    title = typeof input?.command === "string" ? input.command : tool;
    extra.command = title;
  } else if (tool === "read") {
    kind = "file_read"; title = `读取 ${path}`.trim();
  } else if (["grep", "find", "ls"].includes(tool)) {
    kind = "file_search"; title = [tool === "ls" ? "列出目录" : "搜索", pattern, path].filter(Boolean).join(" ");
  } else if (tool === "edit") {
    kind = "patch"; title = `编辑 ${path}`.trim();
    extra.changes = [{ path, kind: "update", diff: details?.patch || details?.diff }];
  } else if (tool === "write") {
    kind = "file_write"; title = `写入 ${path}`.trim();
  }
  return { ...activity, kind, title, output, metadata: extra };
}
