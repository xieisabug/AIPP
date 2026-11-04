import { Blocks } from "lucide-react";

import { JsonDisplay } from "@/components/McpToolCall";
import type { AgentActivityEvent } from "@/data/Conversation";
import { AgentActivityShell } from "./AgentActivityShell";

interface ToolCallActivityCardProps {
  activity: AgentActivityEvent;
}

interface ToolCallMetadata {
  server?: string;
  tool?: string;
}

function toDisplayString(value: unknown): string {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

/**
 * 工具调用活动卡片：形态对齐 MCP 工具卡片，头部为 服务-工具，
 * 展开后分区展示调用参数、执行结果与错误。
 */
export function ToolCallActivityCard({ activity }: ToolCallActivityCardProps) {
  const metadata = (activity.metadata ?? {}) as ToolCallMetadata;
  const parameters = activity.input ?? (activity.metadata as Record<string, unknown> | null)?.arguments;

  return (
    <AgentActivityShell
      icon={<Blocks className="h-4 w-4 flex-shrink-0" />}
      sourceLabel={metadata.server ?? "Codex"}
      title={metadata.tool ?? activity.title ?? "工具调用"}
      status={activity.status}
    >
      {parameters != null && (
        <div className="max-w-full overflow-hidden">
          <span className="text-xs font-medium mb-1 text-muted-foreground">
            参数:
          </span>
          <JsonDisplay
            content={toDisplayString(parameters)}
            maxHeight="120px"
            className="mt-1"
          />
        </div>
      )}

      {activity.output && (
        <div className="max-w-full overflow-hidden">
          <span className="text-xs font-medium mb-1 text-muted-foreground">
            结果:
          </span>
          <JsonDisplay content={activity.output} maxHeight="288px" className="mt-1" />
        </div>
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
