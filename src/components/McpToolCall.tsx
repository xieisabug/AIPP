import React, { useState, useCallback, useMemo, useEffect, useRef, useLayoutEffect } from "react";
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

interface McpToolCallProps {
    serverName?: string;
    toolName?: string;
    parameters?: string;
    conversationId?: number;
    messageId?: number;
    callId?: number; // If provided, this is an existing call
    mcpToolCallStates?: Map<number, MCPToolCallUpdateEvent>; // Global MCP states
}

type ExecutionState = "idle" | "pending" | "executing" | "success" | "failed";

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
                    className="flex items-center gap-1 bg-green-100 text-green-800 border-green-200 ml-3"
                >
                    <CheckCircle className="h-3 w-3" />
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
    conversationId,
    messageId,
    callId,
    mcpToolCallStates,
}) => {
    const [toolCallId, setToolCallId] = useState<number | null>(callId || null);

    const metaOverride = toolCallId && mcpToolCallStates ? mcpToolCallStates.get(toolCallId) : undefined;
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

    const [executionState, setExecutionState] = useState<ExecutionState>("idle");
    const [executionResult, setExecutionResult] = useState<string | null>(null);
    const [executionError, setExecutionError] = useState<string | null>(null);
    // 默认展开：新工具调用默认展开，历史调用根据状态决定
    const [isExpanded, setIsExpanded] = useState<boolean>(!callId);
    // 自动收起定时器引用
    const collapseTimerRef = useRef<NodeJS.Timeout | null>(null);
    // 移除前端自动执行，避免与后端 detect_and_process_mcp_calls 的自动执行叠加

    // 监听全局MCP状态变化
    useEffect(() => {
        if (!mcpToolCallStates) return;

        if (!toolCallId) {
            console.log("[MCP] McpToolCall missing toolCallId; waiting for resolution", {
                conversationId,
                messageId,
                serverName,
                toolName,
                knownIds: Array.from(mcpToolCallStates.keys()),
            });
            return;
        }

        if (mcpToolCallStates.has(toolCallId)) {
            const globalState = mcpToolCallStates.get(toolCallId)!;
            console.log(`McpToolCall ${toolCallId} received global state update:`, globalState);

            // 同步全局状态到本地状态
            switch (globalState.status) {
                case "pending":
                    setExecutionState("pending");
                    setIsExpanded(true); // 待执行的调用默认展开
                    break;
                case "executing":
                    setExecutionState("executing");
                    setIsExpanded(true); // 执行中的调用默认展开
                    break;
                case "success":
                    setExecutionState("success");
                    setExecutionResult(globalState.result || null);
                    setExecutionError(null);
                    // 成功后不改变展开状态，保持用户的选择或使用3秒自动收起逻辑
                    break;
                case "failed":
                    setExecutionState("failed");
                    // 检查是否为用户主动停止
                    if (globalState.error?.includes("Stopped by user")) {
                        setExecutionError("用户已停止");
                    } else {
                        setExecutionError(globalState.error || null);
                    }
                    setExecutionResult(null);
                    setIsExpanded(true); // 失败的调用默认展开，方便查看错误
                    break;
            }
        } else {
            console.log(`[MCP] McpToolCall ${toolCallId} no match in map`, {
                mapKeys: Array.from(mcpToolCallStates.keys()),
            });
        }
    }, [mcpToolCallStates, toolCallId, conversationId, messageId, serverName, toolName]);

    // 检查执行状态
    const isFailed = executionState === "failed";
    const isExecuting = executionState === "executing";
    const canExecute = executionState === "idle" || executionState === "pending" || executionState === "failed"; // idle/pending/failed 状态都可以执行
    const isRunning = executionState === "executing"; // 只有 executing 才显示闪亮边框

    // 如果提供了 callId，尝试获取已有的执行结果
    useEffect(() => {
        if (callId && executionState === "idle") {
            const fetchExistingResult = async () => {
                try {
                    const result = await invoke<MCPToolCall>("get_mcp_tool_call", {
                        callId: callId,
                    });

                    if (result.status === "success" && result.result) {
                        setExecutionResult(result.result);
                        setExecutionState("success");
                        setIsExpanded(false); // 历史成功的调用默认收起
                    } else if (result.status === "failed" && result.error) {
                        setExecutionError(result.error);
                        setExecutionState("failed");
                    }
                } catch (error) {
                    console.warn("Failed to fetch existing tool call result:", error);
                }
            };

            fetchExistingResult();
        }
    }, [callId, executionState]);

    // 如果没有 callId，尝试根据消息参数查询是否存在相关的工具调用记录
    useEffect(() => {
        if (!toolCallId && conversationId && messageId && executionState === "idle") {
            const findExistingToolCall = async () => {
                try {
                    const allCalls = await invoke<MCPToolCall[]>("get_mcp_tool_calls_by_conversation", {
                        conversationId: conversationId,
                    });

                    // 查找匹配的工具调用（相同的消息ID、服务器名和工具名）
                    const matchingCall = allCalls.find(
                        (call) =>
                            call.message_id === messageId &&
                            call.server_name === serverName &&
                            call.tool_name === toolName &&
                            call.parameters === parameters
                    );

                    if (matchingCall) {
                        console.log("[MCP] matched tool call by message/server/tool/parameters", matchingCall);
                        setToolCallId(matchingCall.id);

                        if (matchingCall.status === "success" && matchingCall.result) {
                            setExecutionResult(matchingCall.result);
                            setExecutionState("success");
                            setIsExpanded(false); // 历史成功的调用默认收起
                        } else if (matchingCall.status === "failed" && matchingCall.error) {
                            setExecutionError(matchingCall.error);
                            setExecutionState("failed");
                            setIsExpanded(true); // 失败的调用默认展开，方便查看错误
                        } else if (matchingCall.status === "executing") {
                            setExecutionState("executing");
                            setIsExpanded(true); // 执行中的调用默认展开，显示进度
                        } else if (matchingCall.status === "pending") {
                            setExecutionState("pending");
                            setIsExpanded(true); // 待执行的调用默认展开，显示状态
                        }
                    } else {
                        console.log("[MCP] no matching tool call found for message", {
                            conversationId,
                            messageId,
                            serverName,
                            toolName,
                            parameters,
                            allCallIds: allCalls.map((c) => ({ id: c.id, message_id: c.message_id, status: c.status })),
                        });
                    }
                } catch (error) {
                    console.warn("Failed to find existing tool call:", error);
                }
            };

            findExistingToolCall();
        }
    }, [toolCallId, callId, conversationId, messageId, serverName, toolName, parameters, executionState, mcpToolCallStates]);

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

            let currentCallId = toolCallId;

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
                setToolCallId(currentCallId);
            }

            // Execute the tool call
            const result = await invoke<MCPToolCall>("execute_mcp_tool_call", {
                callId: currentCallId,
                triggerContinuation: false, // 手动执行不触发续写
            });

            if (result.status === "success" && result.result) {
                setExecutionResult(result.result);
                setExecutionState("success");
            } else if (result.status === "failed" && result.error) {
                setExecutionError(result.error);
                setExecutionState("failed");
            }
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : "执行失败";
            setExecutionError(errorMessage);
            setExecutionState("failed");
        }
    }, [conversationId, messageId, serverName, toolName, parameters, toolCallId]);

    const handleStop = useCallback(async () => {
        if (!toolCallId) {
            console.error("Cannot stop: no tool call ID");
            return;
        }

        try {
            await invoke("stop_mcp_tool_call", { callId: toolCallId });
            // 状态会通过 mcp_tool_call_update 事件自动更新
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : "停止失败";
            console.error("Failed to stop tool call:", errorMessage);
            setExecutionError(errorMessage);
            setExecutionState("failed");
        }
    }, [toolCallId]);

    const handleContinueWithError = useCallback(async () => {
        if (!toolCallId) {
            console.error("Cannot continue: no tool call ID");
            return;
        }

        try {
            await invoke("continue_with_error", { callId: toolCallId });
            // 继续对话，状态保持为 failed
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : "继续失败";
            console.error("Failed to continue with error:", errorMessage);
            setExecutionError(errorMessage);
        }
    }, [toolCallId]);

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
            {isRunning && (
                <ShineBorder
                    shineColor={DEFAULT_SHINE_BORDER_CONFIG.shineColor}
                    borderWidth={DEFAULT_SHINE_BORDER_CONFIG.borderWidth}
                    duration={DEFAULT_SHINE_BORDER_CONFIG.duration}
                />
            )}
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-2 text-sm min-w-0 flex-1">
                    <Blocks className="h-4 w-4 flex-shrink-0" />
                    <span className="truncate">{displayServerName}</span>
                    <span className="text-xs font-bold text-muted-foreground flex-shrink-0"> - </span>
                    <span className="truncate">{displayToolName}</span>
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
                    {!isExpanded && canExecute && (
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
                    {!isExpanded && isFailed && (
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
                    {canExecute && (
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
                                    {isFailed && (
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
