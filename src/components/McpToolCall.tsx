import React, { createContext, useState, useCallback, useMemo, useEffect, useRef, useLayoutEffect, useContext } from "react";
import { Play, Loader2, CheckCircle, XCircle, Blocks, ChevronDown, ChevronUp, RotateCcw, Square, ArrowRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ShineBorder } from "@/components/magicui/shine-border";
import { DEFAULT_SHINE_BORDER_CONFIG } from "@/utils/shineConfig";
import { invoke } from "@tauri-apps/api/core";
import { MCPToolCall } from "@/data/MCPToolCall";
import { MCPToolCallUpdateEvent } from "@/data/Conversation";
import { useAntiLeakage } from "@/contexts/AntiLeakageContext";
import { maskToolCall } from "@/utils/antiLeakage";
import { getErrorMessage } from "@/utils/error";

interface McpToolCallProps {
    serverName?: string;
    toolName?: string;
    parameters?: string;
    llmCallId?: string;
    conversationId?: number;
    messageId?: number;
    callId?: number; // If provided, this is an existing call
    mcpToolCallStates?: Map<number, MCPToolCallUpdateEvent>; // Global MCP states
    shiningMcpCallId?: number | null;
    isLastCall?: boolean; // 是否是消息中的最后一个工具调用
    isStreaming?: boolean; // 流式工具调用（LLM正在生成参数）
}

type ExecutionState = "idle" | "pending" | "executing" | "success" | "failed" | "streaming";

const ToolErrorContinueContext = createContext(true);

export const ToolErrorContinueProvider = ToolErrorContinueContext.Provider;

const useToolErrorContinueEnabled = () => useContext(ToolErrorContinueContext);

const JsonDisplay: React.FC<{ content: string; maxHeight?: string; className?: string }> = ({
    content,
    maxHeight = "120px",
    className = "",
}) => {
    const formattedJson = useMemo(() => {
        try {
            const parsed = JSON.parse(content);
            return JSON.stringify(parsed, null, 2);
        } catch {
            return content;
        }
    }, [content]);

    return (
        <div className={`${className} overflow-auto`} style={{ maxHeight: maxHeight }}>
            <pre className="text-xs font-mono p-2 whitespace-pre-wrap break-words mt-0 mb-0 bg-muted text-foreground rounded-md">{formattedJson}</pre>
        </div>
    );
};

const StatusIndicator: React.FC<{ state: ExecutionState }> = ({ state }) => {
    switch (state) {
        case "idle":
            return null;
        case "streaming":
            return (
                <Badge variant="secondary" className="flex items-center gap-1 ml-3">
                    <Loader2 className="h-3 w-3 animate-spin" />
                    生成中
                </Badge>
            );
        case "pending":
            return (
                <Badge variant="outline" className="flex items-center gap-1 ml-3">
                    待执行
                </Badge>
            );
        case "executing":
            return (
                <Badge variant="secondary" className="flex items-center gap-1 ml-3">
                    <Loader2 className="h-3 w-3 animate-spin" />
                    执行中
                </Badge>
            );
        case "success":
            return (
                <Badge
                    variant="default"
                    className="flex items-center gap-1 bg-success text-success-foreground border-success-border ml-3"
                >
                    <CheckCircle className="h-3 w-3 text-success-foreground" />
                    成功
                </Badge>
            );
        case "failed":
            return (
                <Badge variant="destructive" className="flex items-center gap-1 ml-3">
                    <XCircle className="h-3 w-3" />
                    失败
                </Badge>
            );
        default:
            return null;
    }
};

const McpToolCall: React.FC<McpToolCallProps> = ({
    serverName = "未知服务器",
    toolName = "未知工具",
    parameters = "{}",
    llmCallId,
    conversationId,
    messageId,
    callId,
    mcpToolCallStates,
    shiningMcpCallId = null,
    isLastCall = true, // 默认为 true，向后兼容
    isStreaming = false, // 默认非流式
}) => {
    const continueOnToolErrorEnabled = useToolErrorContinueEnabled();
    const [createdCallId, setCreatedCallId] = useState<number | null>(null);
    const matchedStateByLlmCallId = useMemo(() => {
        if (!mcpToolCallStates || !llmCallId || callId || createdCallId) {
            return undefined;
        }
        for (const state of mcpToolCallStates.values()) {
            if (state.llm_call_id === llmCallId) {
                return state;
            }
        }
        return undefined;
    }, [mcpToolCallStates, llmCallId, callId, createdCallId]);
    const effectiveCallId = callId ?? createdCallId ?? matchedStateByLlmCallId?.call_id ?? null;
    const metaOverride = (effectiveCallId && mcpToolCallStates ? mcpToolCallStates.get(effectiveCallId) : undefined) ?? matchedStateByLlmCallId;
    const effectiveServerName = metaOverride?.server_name ?? serverName;
    const effectiveToolName = metaOverride?.tool_name ?? toolName;
    const effectiveParameters = metaOverride?.parameters ?? parameters;

    // 防泄露模式
    const { enabled: antiLeakageEnabled, isRevealed } = useAntiLeakage();
    const shouldMask = antiLeakageEnabled && !isRevealed;

    // 脱敏处理
    const maskedData = useMemo(() => {
        if (!shouldMask) {
            return { serverName: effectiveServerName, toolName: effectiveToolName, parameters: effectiveParameters };
        }
        return maskToolCall(effectiveServerName, effectiveToolName, effectiveParameters);
    }, [shouldMask, effectiveServerName, effectiveToolName, effectiveParameters]);

    const displayServerName = maskedData.serverName;
    const displayToolName = maskedData.toolName;
    const displayParameters = maskedData.parameters;
    const headerTitle = `${displayServerName} - ${displayToolName}`;

    const [executionState, setExecutionState] = useState<ExecutionState>(isStreaming ? "streaming" : "idle");
    const [executionResult, setExecutionResult] = useState<string | null>(null);
    const [executionError, setExecutionError] = useState<string | null>(null);
    // 默认展开：流式调用和新工具调用默认展开，历史调用根据状态决定
    const [isExpanded, setIsExpanded] = useState<boolean>(isStreaming || !callId);
    // 自动收起定时器引用
    const collapseTimerRef = useRef<NodeJS.Timeout | null>(null);
    // 移除前端自动执行，避免与后端 detect_and_process_mcp_calls 的自动执行叠加

    // 监听全局MCP状态变化
    useEffect(() => {
        if (!mcpToolCallStates) return;

        if (!effectiveCallId) {
            console.log("[MCP] McpToolCall missing callId; waiting for streamed call_id", {
                conversationId,
                messageId,
                serverName,
                toolName,
                llmCallId,
                knownIds: Array.from(mcpToolCallStates.keys()),
            });
            return;
        }

        if (mcpToolCallStates.has(effectiveCallId)) {
            const globalState = mcpToolCallStates.get(effectiveCallId)!;
            console.log(`McpToolCall ${effectiveCallId} received global state update:`, globalState);

            // 同步全局状态到本地状态
            switch (globalState.status) {
                case "pending":
                    setExecutionState("pending");
                    setExecutionResult(null);
                    setExecutionError(null);
                    setIsExpanded(true); // 待执行的调用默认展开
                    break;
                case "executing":
                    setExecutionState("executing");
                    setExecutionResult(null);
                    setExecutionError(null);
                    setIsExpanded(true); // 执行中的调用默认展开
                    break;
                case "success":
                    setExecutionState("success");
                    setExecutionResult((prev) => globalState.result ?? prev ?? null);
                    setExecutionError(null);
                    // 成功后不改变展开状态，保持用户的选择或使用3秒自动收起逻辑
                    break;
                case "failed":
                    setExecutionState("failed");
                    // 检查是否为用户主动停止
                    if (globalState.error?.includes("Stopped by user")) {
                        setExecutionError("用户已停止");
                    } else {
                        setExecutionError(globalState.error || "执行失败");
                    }
                    setExecutionResult(null);
                    setIsExpanded(!continueOnToolErrorEnabled); // 开启失败继续时，失败调用会自动续写并收起
                    break;
                case "unknown":
                default:
                    console.log(`[MCP] McpToolCall ${effectiveCallId} ignoring transient unknown state`, globalState);
                    break;
            }
        } else {
            console.log(`[MCP] McpToolCall ${effectiveCallId} no match in map`, {
                mapKeys: Array.from(mcpToolCallStates.keys()),
            });
        }
    }, [mcpToolCallStates, effectiveCallId, conversationId, messageId, serverName, toolName, llmCallId, continueOnToolErrorEnabled]);

    // 检查执行状态
    const isFailed = executionState === "failed";
    const isExecuting = executionState === "executing";
    const canExecute = executionState === "idle" || executionState === "pending" || executionState === "failed"; // idle/pending/failed 状态都可以执行
    const shouldHideFailedActions = isFailed && continueOnToolErrorEnabled;
    const canShowExecutionActions = canExecute && !shouldHideFailedActions;
    const isRunning = effectiveCallId !== null && shiningMcpCallId === effectiveCallId; // 闪亮由全局 shine snapshot 决定

    // 如果提供了 callId，尝试获取已有的执行结果
    useEffect(() => {
        if (effectiveCallId && executionState === "idle") {
            const fetchExistingResult = async () => {
                try {
                    const result = await invoke<MCPToolCall>("get_mcp_tool_call", {
                        callId: effectiveCallId,
                    });

                    if (result.status === "success") {
                        setExecutionResult((prev) => result.result ?? prev ?? null);
                        setExecutionError(null);
                        setExecutionState("success");
                        if (result.result) {
                            setIsExpanded(false); // 历史成功默认收起
                        }
                    } else if (result.status === "failed") {
                        setExecutionError(result.error || "执行失败");
                        setExecutionResult(null);
                        setExecutionState("failed");
                        setIsExpanded(!continueOnToolErrorEnabled);
                    } else if (result.status === "executing") {
                        setExecutionState("executing");
                        setExecutionResult(null);
                        setExecutionError(null);
                        setIsExpanded(true);
                    } else if (result.status === "pending") {
                        setExecutionState("pending");
                        setExecutionResult(null);
                        setExecutionError(null);
                        setIsExpanded(true);
                    }
                } catch (error) {
                    console.warn("Failed to fetch existing tool call result:", error);
                }
            };

            fetchExistingResult();
        }
    }, [effectiveCallId, executionState, continueOnToolErrorEnabled]);

    useEffect(() => {
        if (executionState === "failed") {
            setIsExpanded(!continueOnToolErrorEnabled);
        }
    }, [executionState, continueOnToolErrorEnabled]);

    // 成功后3秒自动收起
    useEffect(() => {
        // 清除之前的定时器
        if (collapseTimerRef.current) {
            clearTimeout(collapseTimerRef.current);
            collapseTimerRef.current = null;
        }

        // 只有当状态变为 success 时才启动定时器
        if (executionState === "success") {
            collapseTimerRef.current = setTimeout(() => {
                setIsExpanded(false);
            }, 3000);
        }

        return () => {
            if (collapseTimerRef.current) {
                clearTimeout(collapseTimerRef.current);
            }
        };
    }, [executionState]);

    // 注意：后端 `detect_and_process_mcp_calls` 已根据助手配置自动执行，这里不再做自动执行

    // 展开/收起动画相关
    const contentRef = useRef<HTMLDivElement>(null);
    const innerContentRef = useRef<HTMLDivElement>(null);
    const [contentHeight, setContentHeight] = useState<number>(0);

    // 计算内容高度用于动画（使用内部容器的高度）
    useLayoutEffect(() => {
        if (innerContentRef.current) {
            const resizeObserver = new ResizeObserver((entries) => {
                for (const entry of entries) {
                    setContentHeight(entry.contentRect.height);
                }
            });
            resizeObserver.observe(innerContentRef.current);
            // 初始设置高度
            setContentHeight(innerContentRef.current.offsetHeight);
            return () => resizeObserver.disconnect();
        }
    }, []);

    // 切换展开/收起状态，同时清除自动收起的定时器
    const handleToggleExpand = useCallback(() => {
        if (collapseTimerRef.current) {
            clearTimeout(collapseTimerRef.current);
            collapseTimerRef.current = null;
        }
        setIsExpanded((prev) => !prev);
    }, []);

    const handleExecute = useCallback(async () => {
        if (!conversationId) {
            console.error("conversation_id is required for execution");
            return;
        }

        try {
            setExecutionState("executing");
            setExecutionResult(null);
            setExecutionError(null);

            let currentCallId = effectiveCallId;

            // Create tool call if it doesn't exist
            if (!currentCallId) {
                const createdCall = await invoke<MCPToolCall>("create_mcp_tool_call", {
                    conversationId: conversationId,
                    messageId: messageId,
                    serverName: serverName,
                    toolName: toolName,
                    parameters,
                });
                currentCallId = createdCall.id;
                setCreatedCallId(currentCallId);
            }

            // Execute the tool call
            // 只有当这是消息中最后一个工具调用时才触发续写
            const result = await invoke<MCPToolCall>("execute_mcp_tool_call", {
                callId: currentCallId,
                triggerContinuation: isLastCall,
            });

            if (result.status === "success") {
                setExecutionResult((prev) => result.result ?? prev ?? null);
                setExecutionError(null);
                setExecutionState("success");
            } else if (result.status === "failed") {
                setExecutionError(result.error || "执行失败");
                setExecutionResult(null);
                setExecutionState("failed");
                setIsExpanded(!continueOnToolErrorEnabled);
            } else if (result.status === "pending") {
                setExecutionState("pending");
                setExecutionResult(null);
                setExecutionError(null);
            } else if (result.status === "executing") {
                setExecutionState("executing");
                setExecutionResult(null);
                setExecutionError(null);
            }
        } catch (error) {
            const errorMessage = getErrorMessage(error) || "执行失败";
            setExecutionError(errorMessage);
            setExecutionState("failed");
            setIsExpanded(!continueOnToolErrorEnabled);
        }
    }, [conversationId, messageId, serverName, toolName, parameters, effectiveCallId, isLastCall, continueOnToolErrorEnabled]);

    const handleStop = useCallback(async () => {
        if (!effectiveCallId) {
            console.error("Cannot stop: no tool call ID");
            return;
        }

        try {
            await invoke("stop_mcp_tool_call", { callId: effectiveCallId });
            // 状态会通过 mcp_tool_call_update 事件自动更新
        } catch (error) {
            const errorMessage = getErrorMessage(error) || "停止失败";
            console.error("Failed to stop tool call:", errorMessage);
            setExecutionError(errorMessage);
            setExecutionState("failed");
        }
    }, [effectiveCallId]);

    const handleContinueWithError = useCallback(async () => {
        if (!effectiveCallId) {
            console.error("Cannot continue: no tool call ID");
            return;
        }

        try {
            await invoke("continue_with_error", { callId: effectiveCallId, errorMessage: executionError });
            // 继续对话，状态保持为 failed
        } catch (error) {
            const errorMessage = getErrorMessage(error) || "继续失败";
            console.error("Failed to continue with error:", errorMessage);
            setExecutionError(errorMessage);
        }
    }, [effectiveCallId, executionError]);

    const renderResult = () => {
        // 防泄露模式：结果也需要脱敏
        const displayResult = shouldMask && executionResult ? "******" : executionResult;
        const displayError = shouldMask && executionError ? "******" : executionError;

        if (displayResult) {
            return (
                <div className="mt-2">
                    <span className="text-xs text-muted-foreground">结果:</span>
                    <JsonDisplay content={displayResult} maxHeight="288px" className="mt-1" />
                </div>
            );
        }

        if (displayError) {
            return (
                <div className="mt-2">
                    <span className="text-xs text-muted-foreground">错误:</span>
                    <JsonDisplay content={displayError} maxHeight="200px" className="mt-1" />
                </div>
            );
        }

        return null;
    };

    return (
        <div className="w-full max-w-[600px] my-1 p-2 border border-border rounded-md bg-card overflow-hidden relative">
            {(isRunning || isStreaming) && (
                <ShineBorder
                    shineColor={DEFAULT_SHINE_BORDER_CONFIG.shineColor}
                    borderWidth={DEFAULT_SHINE_BORDER_CONFIG.borderWidth}
                    duration={DEFAULT_SHINE_BORDER_CONFIG.duration}
                />
            )}
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-2 text-sm min-w-0 flex-1" title={headerTitle}>
                    <Blocks className="h-4 w-4 flex-shrink-0" />
                    <span className="truncate" title={displayServerName}>{displayServerName}</span>
                    <span className="text-xs font-bold text-muted-foreground flex-shrink-0"> - </span>
                    <span className="truncate" title={displayToolName}>{displayToolName}</span>
                </div>
                <div className="flex items-center gap-1 flex-shrink-0">
                    <StatusIndicator state={executionState} />
                    {isExecuting && (
                        <Button
                            onClick={handleStop}
                            size="sm"
                            variant="ghost"
                            className="h-7 w-7 p-0 flex-shrink-0 text-destructive"
                            title="停止"
                        >
                            <Square className="h-3 w-3 fill-current" />
                        </Button>
                    )}
                    {!isExpanded && canShowExecutionActions && (
                        <Button
                            onClick={handleExecute}
                            disabled={isExecuting}
                            size="sm"
                            variant="ghost"
                            className="h-7 w-7 p-0 flex-shrink-0"
                            title={isFailed ? "重新执行" : "执行"}
                        >
                            {isExecuting ? (
                                <Loader2 className="h-3 w-3 animate-spin" />
                            ) : isFailed ? (
                                <RotateCcw className="h-3 w-3" />
                            ) : (
                                <Play className="h-3 w-3" />
                            )}
                        </Button>
                    )}
                    {!isExpanded && isFailed && !shouldHideFailedActions && (
                        <Button
                            onClick={handleContinueWithError}
                            size="sm"
                            variant="ghost"
                            className="h-7 w-7 p-0 flex-shrink-0"
                            title="以错误继续对话"
                        >
                            <ArrowRight className="h-3 w-3" />
                        </Button>
                    )}
                    <Button
                        onClick={handleToggleExpand}
                        size="sm"
                        variant="ghost"
                        className="h-7 w-7 p-0 flex-shrink-0"
                        title={isExpanded ? "收起详情" : "展开详情"}
                    >
                        {isExpanded ? <ChevronUp className="h-3 w-3" /> : <ChevronDown className="h-3 w-3" />}
                    </Button>
                </div>
            </div>

            {/* 带动画的可折叠内容区域 */}
            <div
                ref={contentRef}
                className="overflow-hidden transition-all duration-300 ease-in-out"
                style={{
                    height: isExpanded ? `${contentHeight}px` : '0px',
                    opacity: isExpanded ? 1 : 0,
                }}
            >
                <div ref={innerContentRef} className="mt-2 space-y-2 max-w-full overflow-hidden">
                    <div className="max-w-full overflow-hidden">
                        <span className="text-xs font-medium mb-1 text-muted-foreground">参数:</span>
                        <JsonDisplay content={displayParameters} maxHeight="120px" className="mt-1" />
                    </div>
                    {canShowExecutionActions && (
                        <div className="flex items-center gap-2">
                            {isExecuting ? (
                                <>
                                    <Button
                                        onClick={handleStop}
                                        size="sm"
                                        variant="ghost"
                                        className="flex items-center gap-1 h-7 text-xs text-destructive"
                                        title="停止"
                                    >
                                        <Square className="h-3 w-3 fill-current" />
                                        停止
                                    </Button>
                                </>
                            ) : (
                                <>
                                    <Button
                                        onClick={handleExecute}
                                        size="sm"
                                        className="flex items-center gap-1 h-7 text-xs"
                                    >
                                        {isFailed ? (
                                            <RotateCcw className="h-3 w-3" />
                                        ) : (
                                            <Play className="h-3 w-3" />
                                        )}
                                        {isFailed ? "重新执行" : "执行"}
                                    </Button>
                                    {isFailed && !shouldHideFailedActions && (
                                        <Button
                                            onClick={handleContinueWithError}
                                            size="sm"
                                            variant="outline"
                                            className="flex items-center gap-1 h-7 text-xs"
                                        >
                                            <ArrowRight className="h-3 w-3" />
                                            以错误继续
                                        </Button>
                                    )}
                                </>
                            )}
                        </div>
                    )}
                    <div className="max-w-full overflow-hidden">{renderResult()}</div>
                </div>
            </div>
        </div>
    );
};

export default McpToolCall;
