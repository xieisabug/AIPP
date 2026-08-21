import { ChevronDown, ChevronUp } from "lucide-react";
import React, { useCallback, useEffect, useRef, useState } from "react";

import { StatusIndicator } from "@/components/McpToolCall";
import type { ExecutionState } from "@/components/McpToolCall";
import {
  MotionDetails,
  MotionStatusSlot,
  MotionToolCard,
} from "@/components/mcp-tool-components/McpToolMotion";
import { ShineBorder } from "@/components/magicui/shine-border";
import { Button } from "@/components/ui/button";
import { DEFAULT_SHINE_BORDER_CONFIG } from "@/utils/shineConfig";

/** Agent 活动状态到卡片执行态的映射，未知状态按执行中展示 */
export function toExecutionState(status: string): ExecutionState {
  switch (status) {
    case "pending":
      return "pending";
    case "success":
      return "success";
    case "failed":
      return "failed";
    case "cancelled":
      return "cancelled";
    case "executing":
    default:
      return "executing";
  }
}

interface AgentActivityShellProps {
  icon: React.ReactNode;
  sourceLabel: string;
  title: string;
  status: string;
  headerExtra?: React.ReactNode;
  children?: React.ReactNode;
}

/**
 * Agent 活动卡片的共享外壳。
 *
 * 复用 MCP 工具卡片的卡片容器、边框光效与展开交互，保证整套活动卡片与
 * 既有 MCP 工具调用 UI 风格一致。卡片是只读的，不提供执行按钮。
 */
export function AgentActivityShell({
  icon,
  sourceLabel,
  title,
  status,
  headerExtra,
  children,
}: AgentActivityShellProps) {
  const executionState = toExecutionState(status);
  const expandable = children != null;

  const [isExpanded, setIsExpanded] = useState(false);
  const userExpandedRef = useRef<boolean | null>(null);

  const handleToggleExpand = useCallback(() => {
    setIsExpanded((prev) => {
      const next = !prev;
      userExpandedRef.current = next;
      return next;
    });
  }, []);

  useEffect(() => {
    if (!expandable) return;
    // 失败/运行中的任务自动展开以便查看，成功后自动收起（不覆盖用户手动操作）
    if (
      executionState === "failed" ||
      executionState === "executing" ||
      executionState === "pending"
    ) {
      setIsExpanded(true);
      return;
    }
    if (
      (executionState === "success" || executionState === "cancelled") &&
      userExpandedRef.current == null
    ) {
      const timer = setTimeout(() => setIsExpanded(false), 3000);
      return () => clearTimeout(timer);
    }
  }, [executionState, expandable]);

  const isRunning =
    executionState === "executing" || executionState === "pending";

  return (
    <MotionToolCard>
      {isRunning && (
        <ShineBorder
          shineColor={DEFAULT_SHINE_BORDER_CONFIG.shineColor}
          borderWidth={DEFAULT_SHINE_BORDER_CONFIG.borderWidth}
          duration={DEFAULT_SHINE_BORDER_CONFIG.duration}
        />
      )}
      <div className="flex items-center justify-between">
        <div
          className="flex items-center gap-2 text-sm min-w-0 flex-1"
          title={title}
        >
          {icon}
          <span className="truncate text-muted-foreground" title={sourceLabel}>
            {sourceLabel}
          </span>
          <span className="text-xs font-bold text-muted-foreground flex-shrink-0">
            {" "}
            -{" "}
          </span>
          <span className="truncate">{title}</span>
        </div>
        <div className="flex items-center gap-1 flex-shrink-0">
          {headerExtra}
          <MotionStatusSlot stateKey={executionState} present>
            <StatusIndicator state={executionState} />
          </MotionStatusSlot>
          {expandable && (
            <Button
              onClick={handleToggleExpand}
              size="sm"
              variant="ghost"
              className="h-7 w-7 p-0 flex-shrink-0"
              title={isExpanded ? "收起详情" : "展开详情"}
            >
              {isExpanded ? (
                <ChevronUp className="h-3 w-3" />
              ) : (
                <ChevronDown className="h-3 w-3" />
              )}
            </Button>
          )}
        </div>
      </div>

      {expandable && (
        <MotionDetails show={isExpanded}>
          <div className="mt-2 space-y-2 max-w-full overflow-hidden">
            {children}
          </div>
        </MotionDetails>
      )}
    </MotionToolCard>
  );
}
