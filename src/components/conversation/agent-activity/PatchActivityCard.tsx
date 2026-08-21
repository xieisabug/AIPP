import { ChevronDown, FileDiff } from "lucide-react";
import { useState } from "react";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import {
  oneDark,
  oneLight,
} from "react-syntax-highlighter/dist/esm/styles/prism";

import { Badge } from "@/components/ui/badge";
import type { AgentActivityEvent } from "@/data/Conversation";
import { useTheme } from "@/hooks/useTheme";
import { cn } from "@/utils/utils";
import { AgentActivityShell } from "./AgentActivityShell";

interface PatchActivityCardProps {
  activity: AgentActivityEvent;
}

interface FileChange {
  path?: string;
  kind?: unknown;
  diff?: string;
}

interface PatchMetadata {
  changes?: FileChange[];
}

/** kind 可能是字符串或 { "type": "add" } 之类的对象，统一归一化 */
function normalizeKind(kind: unknown): string {
  if (typeof kind === "string") return kind;
  if (kind && typeof kind === "object") {
    const record = kind as Record<string, unknown>;
    if (typeof record.type === "string") return record.type;
    const keys = Object.keys(record);
    if (keys.length > 0) return keys[0];
  }
  return "update";
}

function kindLabel(kind: string): string {
  switch (kind) {
    case "add":
      return "新增";
    case "delete":
      return "删除";
    default:
      return "修改";
  }
}

function kindVariant(kind: string): "default" | "secondary" | "destructive" {
  switch (kind) {
    case "add":
      return "default";
    case "delete":
      return "destructive";
    default:
      return "secondary";
  }
}

/**
 * 文件变更活动卡片：头部展示涉及文件数，展开后列出文件清单与可折叠 diff。
 */
export function PatchActivityCard({ activity }: PatchActivityCardProps) {
  const metadata = (activity.metadata ?? {}) as PatchMetadata;
  const changes = metadata.changes ?? [];
  const { resolvedTheme } = useTheme();

  return (
    <AgentActivityShell
      icon={<FileDiff className="h-4 w-4 flex-shrink-0" />}
      sourceLabel="Codex"
      title={`文件变更（${changes.length} 个文件）`}
      status={activity.status}
    >
      <div className="space-y-2">
        {changes.map((change, index) => (
          <PatchFileItem
            key={`${change.path ?? "unknown"}-${index}`}
            change={change}
            isDark={resolvedTheme === "dark"}
          />
        ))}
        {changes.length === 0 && activity.output && (
          <pre className="text-xs font-mono whitespace-pre-wrap break-all text-foreground max-h-80 overflow-auto">
            {activity.output}
          </pre>
        )}
      </div>
    </AgentActivityShell>
  );
}

function PatchFileItem({
  change,
  isDark,
}: {
  change: FileChange;
  isDark: boolean;
}) {
  const [showDiff, setShowDiff] = useState(false);
  const kind = normalizeKind(change.kind);

  return (
    <div className="rounded-md border border-border">
      <div
        className={cn(
          "flex items-center gap-2 px-3 py-2",
          change.diff && "cursor-pointer select-none"
        )}
        onClick={change.diff ? () => setShowDiff(!showDiff) : undefined}
      >
        <Badge variant={kindVariant(kind)} className="text-xs flex-shrink-0">
          {kindLabel(kind)}
        </Badge>
        <span className="text-xs font-mono truncate flex-1">
          {change.path ?? "未知文件"}
        </span>
        {change.diff && (
          <ChevronDown
            className={cn(
              "h-3.5 w-3.5 text-muted-foreground transition-transform flex-shrink-0",
              showDiff && "rotate-180"
            )}
          />
        )}
      </div>
      {change.diff && showDiff && (
        <div className="border-t border-border">
          <SyntaxHighlighter
            language="diff"
            style={isDark ? oneDark : oneLight}
            customStyle={{
              margin: 0,
              fontSize: "0.75rem",
              background: "transparent",
            }}
            PreTag="div"
          >
            {change.diff}
          </SyntaxHighlighter>
        </div>
      )}
    </div>
  );
}
