import React, { useState, useEffect } from "react";
import { SubTaskExecutionDetail, SubTaskExecutionSummary, MCPToolCallUI, useSubTaskIcon } from "../../data/SubTask";
import { subTaskService, getStatusColor, getStatusText, formatTokenCount, formatDuration } from "../../services/subTaskService";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from "../ui/dialog";
import { Button } from "../ui/button";
import { Badge } from "../ui/badge";
import { StopCircle, RefreshCw, Clock, Zap, AlertCircle } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "../ui/card";

export interface SubTaskDetailDialogProps {
    isOpen: boolean;
    onClose: () => void;
    execution: SubTaskExecutionSummary;
    onCancel?: (execution_id: number) => void;
}

const SubTaskDetailDialog: React.FC<SubTaskDetailDialogProps> = ({ isOpen, onClose, execution, onCancel }) => {
    const [detail, setDetail] = useState<SubTaskExecutionDetail | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [mcpCalls, setMcpCalls] = useState<MCPToolCallUI[] | null>(null);
    const iconComp = useSubTaskIcon(execution.task_code);

    // Load detailed information when dialog opens
    useEffect(() => {
        if (isOpen && execution) {
            loadDetail();
        }
    }, [isOpen, execution.id]);

    const loadDetail = async () => {
        try {
            setLoading(true);
            setError(null);
            // 使用UI专用的详情获取方法（不需要鉴权）
            const detailData = await subTaskService.getExecutionDetailForUI(execution.id);
            setDetail(detailData);
            // 并行加载 MCP 调用
            if (subTaskService.getMcpCallsForExecution) {
                try {
                    const calls = await subTaskService.getMcpCallsForExecution(execution.id);
                    setMcpCalls(calls);
                } catch (e) {
                    console.warn("加载 MCP 工具调用失败", e);
                    setMcpCalls([]);
                }
            } else {
                setMcpCalls([]);
            }
        } catch (err) {
            setError(err instanceof Error ? err.message : "加载详情失败");
        } finally {
            setLoading(false);
        }
    };

    const handleCancel = async () => {
        if (onCancel && execution.status === "running") {
            try {
                // 取消任务操作交给父组件处理，因为父组件知道合适的source_id
                await onCancel(execution.id);
                onClose();
            } catch (error) {
                console.error("Failed to cancel task:", error);
            }
        }
    };

    const canCancel = execution.status === "running" && onCancel;

    const renderTaskIcon = () => {
        if (!iconComp) return null;
        const size = 16;
        if (React.isValidElement(iconComp)) {
            return (
                <span className="inline-flex items-center justify-center" style={{ lineHeight: 0 }}>
                    {iconComp}
                </span>
            );
        }
        const Comp = iconComp as React.ComponentType<{ className?: string; size?: number }>;
        return <Comp className="text-muted-foreground" size={size} />;
    };

    return (
        <Dialog open={isOpen} onOpenChange={onClose}>
            {/* 移除圆角并添加 webkit 文本清晰修复类 */}
            <DialogContent className="!w-[1000px] !max-w-[95vw] h-[80vh] min-w-[480px] min-h-[480px] flex flex-col overflow-hidden webkit-scroll-fix !rounded-none">
                <DialogHeader>
                    <DialogTitle className="flex items-center gap-2">
                        <div className="flex items-center gap-2">
                            {renderTaskIcon()}
                            <span>{execution.task_name}</span>
                        </div>
                        <Badge className={getStatusColor(execution.status)}>{getStatusText(execution.status)}</Badge>
                    </DialogTitle>
                </DialogHeader>

                {/* 主要滚动容器，添加修复类 */}
                <div className="flex-1 min-h-0 min-w-0 overflow-y-auto webkit-scroll-fix">
                    {loading ? (
                        <div className="flex items-center justify-center h-full">
                            <RefreshCw className="w-6 h-6 animate-spin" />
                            <span className="ml-2">加载中...</span>
                        </div>
                    ) : error ? (
                        <div className="flex items-center justify-center h-full text-destructive">
                            <AlertCircle className="w-6 h-6" />
                            <span className="ml-2">{error}</span>
                        </div>
                    ) : (
                        <div className="h-full w-full">
                            <div className="space-y-4 px-1">
                                {/* Basic Information */}
                                <Card>
                                    <CardHeader>
                                        <CardTitle>基本信息</CardTitle>
                                    </CardHeader>
                                    <CardContent className="space-y-2 min-w-0">
                                        <div className="grid grid-cols-2 gap-4 text-sm">
                                            <div className="min-w-0">
                                                <span className="text-muted-foreground">任务代码:</span>
                                                <span className="ml-2 font-mono break-words">{execution.task_code}</span>
                                            </div>
                                            <div>
                                                <span className="text-muted-foreground">创建时间:</span>
                                                <span className="ml-2">{execution.created_time.toLocaleString()}</span>
                                            </div>
                                            {detail?.started_time && (
                                                <div>
                                                    <span className="text-muted-foreground">开始时间:</span>
                                                    <span className="ml-2">{detail.started_time.toLocaleString()}</span>
                                                </div>
                                            )}
                                            {detail?.finished_time && (
                                                <div>
                                                    <span className="text-muted-foreground">完成时间:</span>
                                                    <span className="ml-2">
                                                        {detail.finished_time.toLocaleString()}
                                                    </span>
                                                </div>
                                            )}
                                        </div>

                                        {/* Duration and Token Information */}
                                        <div className="flex items-center gap-4 pt-2">
                                            <div className="flex items-center gap-1 text-sm" title="任务耗时">
                                                <Clock className="w-4 h-4 text-muted-foreground" />
                                                <span>{formatDuration(detail?.started_time, detail?.finished_time)}</span>
                                            </div>
                                            <div className="flex items-center gap-1 text-sm" title="Token 消耗">
                                                <Zap className="w-4 h-4 text-muted-foreground" />
                                                <span>{formatTokenCount(execution.token_count)}</span>
                                            </div>
                                        </div>
                                    </CardContent>
                                </Card>

                                {/* Task Prompt */}
                                {execution.task_prompt && (
                                    <Card>
                                        <CardHeader>
                                            <CardTitle>任务提示</CardTitle>
                                        </CardHeader>
                                        <CardContent className="min-w-0">
                                            <div className="rounded bg-background border max-h-128 overflow-x-auto overflow-y-auto webkit-scroll-fix">
                                                <pre className="p-3 text-sm font-mono leading-5 whitespace-pre">{execution.task_prompt}</pre>
                                            </div>
                                        </CardContent>
                                    </Card>
                                )}

                                {/* Result Content */}
                                {detail?.result_content && (
                                    <Card>
                                        <CardHeader className="pb-3">
                                            <CardTitle className="text-sm">执行结果</CardTitle>
                                        </CardHeader>
                                        <CardContent className="min-w-0">
                                            <div className="rounded bg-background border max-h-128 overflow-x-auto overflow-y-auto webkit-scroll-fix">
                                                <pre className="p-3 text-sm font-mono leading-5 whitespace-pre">{detail.result_content}</pre>
                                            </div>
                                        </CardContent>
                                    </Card>
                                )}
                                {/* MCP Tool Calls */}
                                {mcpCalls && mcpCalls.length > 0 && (
                                    <Card>
                                        <CardHeader className="pb-3">
                                            <CardTitle className="text-sm">MCP 工具调用</CardTitle>
                                        </CardHeader>
                                        <CardContent className="space-y-3">
                                            {mcpCalls.map((call) => (
                                                <div key={call.id} className="border rounded p-3 text-sm">
                                                    <div className="flex items-center justify-between">
                                                        <div className="font-medium">
                                                            {call.server_name} / {call.tool_name}
                                                        </div>
                                                        <Badge className={getStatusColor(call.status)}>
                                                            {getStatusText(call.status)}
                                                        </Badge>
                                                    </div>
                                                    <div className="mt-2 grid grid-cols-2 gap-2 text-xs text-muted-foreground">
                                                        <div>创建: {call.created_time.toLocaleString()}</div>
                                                        {call.started_time && (
                                                            <div>开始: {call.started_time.toLocaleString()}</div>
                                                        )}
                                                        {call.finished_time && (
                                                            <div>结束: {call.finished_time.toLocaleString()}</div>
                                                        )}
                                                    </div>
                                                    {call.parameters && (
                                                        <div className="mt-2">
                                                            <div className="text-xs text-muted-foreground">参数</div>
                                                            <div className="bg-background border rounded max-h-40 overflow-x-auto overflow-y-auto webkit-scroll-fix">
                                                                <pre className="p-2 text-sm font-mono leading-5 whitespace-pre">{call.parameters}</pre>
                                                            </div>
                                                        </div>
                                                    )}
                                                    {call.result && (
                                                        <div className="mt-2">
                                                            <div className="text-xs text-muted-foreground">结果</div>
                                                            <div className="bg-background border rounded max-h-40 overflow-x-auto overflow-y-auto webkit-scroll-fix">
                                                                <pre className="p-2 text-sm font-mono leading-5 whitespace-pre">{call.result}</pre>
                                                            </div>
                                                        </div>
                                                    )}
                                                    {call.error && (
                                                        <div className="mt-2">
                                                            <div className="text-xs text-muted-foreground">错误</div>
                                                            <div className="bg-background text-destructive border border-destructive/30 rounded max-h-40 overflow-x-auto overflow-y-auto webkit-scroll-fix">
                                                                <pre className="p-2 text-sm font-mono leading-5 whitespace-pre">{call.error}</pre>
                                                            </div>
                                                        </div>
                                                    )}
                                                </div>
                                            ))}
                                        </CardContent>
                                    </Card>
                                )}

                                {/* Error Message */}
                                {detail?.error_message && (
                                    <Card className="border-destructive">
                                        <CardHeader className="pb-3">
                                            <CardTitle className="text-sm text-destructive">错误信息</CardTitle>
                                        </CardHeader>
                                        <CardContent className="min-w-0">
                                            <div className="bg-background border border-destructive/30 rounded max-h-64 overflow-x-auto overflow-y-auto webkit-scroll-fix">
                                                <pre className="p-3 text-sm font-mono leading-5 text-destructive whitespace-pre">{detail.error_message}</pre>
                                            </div>
                                        </CardContent>
                                    </Card>
                                )}

                                {/* MCP Loop Summary */}
                                {detail?.mcp_result_json && (
                                    <Card>
                                        <CardHeader className="pb-3">
                                            <CardTitle className="text-sm">MCP 循环概要</CardTitle>
                                        </CardHeader>
                                        <CardContent className="space-y-2 text-sm">
                                            {(() => {
                                                try {
                                                    const data = JSON.parse(detail.mcp_result_json!);
                                                    return (
                                                        <div className="space-y-2">
                                                            <div>循环轮数: {data.loops}</div>
                                                            <div>
                                                                总调用数:{" "}
                                                                {data.metrics?.totalCalls ?? data.metrics?.total_calls}
                                                            </div>
                                                            <div>
                                                                成功调用:{" "}
                                                                {data.metrics?.successCalls ??
                                                                    data.metrics?.success_calls}
                                                            </div>
                                                            <div>
                                                                失败调用:{" "}
                                                                {data.metrics?.failedCalls ??
                                                                    data.metrics?.failed_calls}
                                                            </div>
                                                            {data.abortReason && (
                                                                <div>终止原因: {data.abortReason}</div>
                                                            )}
                                                        </div>
                                                    );
                                                } catch (e) {
                                                    return (
                                                        <div className="text-muted-foreground">无法解析 MCP 结果</div>
                                                    );
                                                }
                                            })()}
                                        </CardContent>
                                    </Card>
                                )}

                                {/* Model Information */}
                                {detail?.llm_model_name && (
                                    <Card>
                                        <CardHeader className="pb-3">
                                            <CardTitle className="text-sm">模型信息</CardTitle>
                                        </CardHeader>
                                        <CardContent className="space-y-2">
                                            <div className="text-sm">
                                                <span className="text-muted-foreground">模型:</span>
                                                <span className="ml-2 font-mono">{detail.llm_model_name}</span>
                                            </div>
                                            <div className="grid grid-cols-2 gap-4 text-sm">
                                                <div>
                                                    <span className="text-muted-foreground">输入 Tokens:</span>
                                                    <span className="ml-2">
                                                        {formatTokenCount(detail.input_token_count)}
                                                    </span>
                                                </div>
                                                <div>
                                                    <span className="text-muted-foreground">输出 Tokens:</span>
                                                    <span className="ml-2">
                                                        {formatTokenCount(detail.output_token_count)}
                                                    </span>
                                                </div>
                                            </div>
                                        </CardContent>
                                    </Card>
                                )}
                            </div>
                        </div>
                    )}
                </div>

                <DialogFooter className="flex justify-between">
                    <div>
                        {canCancel && (
                            <Button variant="destructive" onClick={handleCancel}>
                                <StopCircle className="w-4 h-4 mr-1" />
                                停止任务
                            </Button>
                        )}
                    </div>
                    <Button onClick={onClose}>关闭</Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
};

export default SubTaskDetailDialog;
