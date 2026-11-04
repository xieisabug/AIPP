import { Terminal } from "lucide-react";

import { JsonDisplay } from "@/components/McpToolCall";
import { Badge } from "@/components/ui/badge";
import type { AgentActivityEvent } from "@/data/Conversation";
import { AgentActivityShell } from "./AgentActivityShell";

interface CommandActivityCardProps {
  activity: AgentActivityEvent;
}

interface CommandMetadata {
  command?: string;
  cwd?: string;
  exitCode?: number | null;
}

/**
 * 终端命令活动卡片：头部显示命令本身，完成后展示 exit code，
 * 展开后以终端样式展示 cwd 与完整输出。
 */
export function CommandActivityCard({ activity }: CommandActivityCardProps) {
  const metadata = (activity.metadata ?? {}) as CommandMetadata;
  const command = metadata.command ?? activity.title ?? "终端命令";
  const exitCode = metadata.exitCode;
  const hasExitCode = activity.status === "success" || activity.status === "failed";

  const headerExtra =
    hasExitCode && exitCode != null ? (
      <Badge
        variant={exitCode === 0 ? "secondary" : "destructive"}
        className="text-xs font-mono"
      >
        exit code: {exitCode}
      </Badge>
    ) : null;

  return (
    <AgentActivityShell
      icon={<Terminal className="h-4 w-4 flex-shrink-0" />}
      sourceLabel="Codex"
      title={command}
      status={activity.status}
      headerExtra={headerExtra}
    >
      {metadata.cwd && (
        <div className="text-xs text-muted-foreground font-mono truncate">
          cwd: {metadata.cwd}
        </div>
      )}

      {activity.output && (
        <pre className="text-xs font-mono whitespace-pre-wrap break-all rounded-md bg-zinc-950 text-zinc-100 p-3 max-h-80 overflow-auto">
          {activity.output}
        </pre>
      )}

      {activity.error && (
        <div className="max-w-full overflow-hidden">
          <span className="text-xs font-medium mb-1 text-muted-foreground">
            错误:
          </span>
          <JsonDisplay content={activity.error} maxHeight="200px" className="mt-1" />
        </div>
      )}
    </AgentActivityShell>
  );
}
