import { invoke } from "@tauri-apps/api/core";
import { emitTo, listen, once } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import {
    Bot,
    Check,
    ExternalLink,
    Loader2,
    PauseCircle,
    Plus,
    RefreshCw,
    X,
} from "lucide-react";

import ConversationUI, {
    ConversationUIRef,
    type InlineInteractionItem,
} from "@/components/ConversationUI";
import UnifiedMarkdown from "@/components/UnifiedMarkdown";
import {
    AcpPermissionDialog,
    OperationPermissionDialog,
} from "@/components/OperationPermissionDialog";
import {
    AskUserQuestionCard,
    PreviewFileCard,
} from "@/components/InlineInteractionCards";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { Textarea } from "@/components/ui/textarea";
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select";
import { AssistantListItem } from "@/data/Assistant";
import {
    ButlerMainLoadResponse,
    ButlerNotificationEvent,
    ButlerTaskDetailResponse,
    ButlerTaskListItem,
    ButlerTaskResultAvailableEvent,
} from "@/data/Butler";
import { useAskUserQuestion, usePreviewFile } from "@/hooks/useInlineInteraction";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import { useTheme } from "@/hooks/useTheme";
import { useAcpPermission, useOperationPermission } from "@/hooks/useOperationPermission";
import { AntiLeakageProvider } from "@/contexts/AntiLeakageContext";
import { pluginRuntime } from "@/services/PluginRuntime";

function safeParseJson<T>(value?: string | null): T | null {
    if (!value) {
        return null;
    }
    try {
        return JSON.parse(value) as T;
    } catch {
        return null;
    }
}

function sortTasks(tasks: ButlerTaskListItem[]): ButlerTaskListItem[] {
    return [...tasks].sort((left, right) => {
        const timeDiff =
            new Date(right.updated_time).getTime() - new Date(left.updated_time).getTime();
        if (timeDiff !== 0) {
            return timeDiff;
        }
        return right.task_conversation_id - left.task_conversation_id;
    });
}

function upsertTask(
    tasks: ButlerTaskListItem[],
    nextTask: ButlerTaskListItem
): ButlerTaskListItem[] {
    const filtered = tasks.filter(
        (task) => task.task_conversation_id !== nextTask.task_conversation_id
    );
    return sortTasks([...filtered, nextTask]);
}

function formatTime(value?: string | null): string {
    if (!value) {
        return "—";
    }
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) {
        return value;
    }
    return date.toLocaleString();
}

function getStatusVariant(
    status: string
): "default" | "destructive" | "secondary" | "outline" {
    switch (status) {
        case "succeeded":
            return "default";
        case "failed":
            return "destructive";
        case "cancelled":
            return "secondary";
        case "running":
            return "outline";
        default:
            return "secondary";
    }
}

function getStatusLabel(status: string) {
    switch (status) {
        case "accepted":
            return "已受理";
        case "running":
            return "执行中";
        case "succeeded":
            return "已完成";
        case "failed":
            return "失败";
        case "cancelled":
            return "已取消";
        default:
            return status;
    }
}

interface FeishuRuntimeStatus {
    butler_enabled: boolean;
    enabled: boolean;
    configured: boolean;
    secret_configured: boolean;
    running: boolean;
    connected: boolean;
    app_id?: string | null;
    base_url?: string | null;
    allow_p2p: boolean;
    allow_group: boolean;
    group_require_mention: boolean;
    last_error?: string | null;
    last_event_at?: string | null;
    last_status_at?: string | null;
    status_detail?: string | null;
    status_text: string;
}

function getFeishuStatusVariant(
    status?: FeishuRuntimeStatus | null
): "default" | "destructive" | "secondary" | "outline" {
    if (!status) {
        return "secondary";
    }
    if (status.last_error) {
        return "destructive";
    }
    if (status.connected) {
        return "default";
    }
    if (status.running) {
        return "outline";
    }
    return "secondary";
}

function getFeishuStatusIcon(
    status?: FeishuRuntimeStatus | null,
    loading?: boolean
) {
    if (loading || (!status && loading !== false)) {
        return <Loader2 className="h-3 w-3 animate-spin" />;
    }
    if (status?.connected) {
        return <Check className="h-3 w-3" />;
    }
    if (status?.running) {
        return <Loader2 className="h-3 w-3 animate-spin" />;
    }
    return <X className="h-3 w-3" />;
}

function ButlerExperimentWindow() {
    useTheme("butler_experiment");
    const { getConfigValue, loadFeatureConfig } = useFeatureConfig();
    const antiLeakageEnabled = getConfigValue("anti_leakage", "enabled") === "true";
    const butlerExperimentEnabled =
        getConfigValue("experimental", "butler_experiment_enabled") === "true";
    const feishuEnabled = getConfigValue("experimental", "butler_feishu_enabled") === "true";
    const showFeishuStatus = butlerExperimentEnabled && feishuEnabled;

    const conversationUIRef = useRef<ConversationUIRef>(null);
    const [pluginList, setPluginList] = useState<any[]>([]);
    const [assistants, setAssistants] = useState<AssistantListItem[]>([]);
    const [mainConversationId, setMainConversationId] = useState<string>("");
    const [mainModelDisplayName, setMainModelDisplayName] = useState<string>("");
    const [tasks, setTasks] = useState<ButlerTaskListItem[]>([]);
    const [selectedTaskId, setSelectedTaskId] = useState<number | null>(null);
    const [selectedTaskDetail, setSelectedTaskDetail] =
        useState<ButlerTaskDetailResponse | null>(null);
    const [loadingMain, setLoadingMain] = useState(true);
    const [loadingTaskDetail, setLoadingTaskDetail] = useState(false);
    const [creatingTask, setCreatingTask] = useState(false);
    const [resettingMainConversation, setResettingMainConversation] = useState(false);
    const [loadingFeishuStatus, setLoadingFeishuStatus] = useState(false);
    const [isTaskDialogOpen, setIsTaskDialogOpen] = useState(false);
    const [isTaskDetailDialogOpen, setIsTaskDetailDialogOpen] = useState(false);
    const [taskTitle, setTaskTitle] = useState("");
    const [taskGoal, setTaskGoal] = useState("");
    const [taskAssistantId, setTaskAssistantId] = useState<string>("");
    const [loadError, setLoadError] = useState<string | null>(null);
    const [feishuStatus, setFeishuStatus] = useState<FeishuRuntimeStatus | null>(null);

    const conversationIdNumber = mainConversationId ? parseInt(mainConversationId, 10) : undefined;
    const permissionConversationIds = useMemo(() => {
        const ids = new Set<number>();
        if (conversationIdNumber !== undefined) {
            ids.add(conversationIdNumber);
        }
        tasks.forEach((task) => {
            ids.add(task.task_conversation_id);
        });
        return Array.from(ids);
    }, [conversationIdNumber, tasks]);

    const {
        pendingRequest,
        isDialogOpen,
        decisionError,
        handleDecision,
    } = useOperationPermission({
        conversationId: conversationIdNumber,
        conversationIds: permissionConversationIds,
    });
    const {
        pendingRequest: pendingAcpRequest,
        isDialogOpen: isAcpDialogOpen,
        decisionError: acpDecisionError,
        handleDecision: handleAcpDecision,
    } = useAcpPermission({
        conversationId: conversationIdNumber,
        conversationIds: permissionConversationIds,
    });
    const {
        pendingRequest: pendingAskUserRequest,
        isDialogOpen: isAskUserDialogOpen,
        viewMode: askUserViewMode,
        completedAnswers: askUserCompletedAnswers,
        readOnly: isAskUserReadOnly,
        callId: askUserCallId,
        messageId: askUserMessageId,
        handleSubmit: handleAskUserSubmit,
        handleCancel: handleAskUserCancel,
    } = useAskUserQuestion({
        conversationId: conversationIdNumber,
    });
    const {
        pendingRequest: pendingPreviewFileRequest,
        isDialogOpen: isPreviewFileDialogOpen,
        callId: previewFileCallId,
        messageId: previewFileMessageId,
        handleOpenChange: handlePreviewFileOpenChange,
    } = usePreviewFile({
        conversationId: conversationIdNumber,
    });

    const inlineInteractionItems: InlineInteractionItem[] = [];
    if (isAskUserDialogOpen && pendingAskUserRequest) {
        inlineInteractionItems.push({
            key: `butler-ask-user-${pendingAskUserRequest.request_id}`,
            callId: askUserCallId,
            messageId: askUserMessageId,
            content: (
                <AskUserQuestionCard
                    request={pendingAskUserRequest}
                    isOpen={isAskUserDialogOpen}
                    viewMode={askUserViewMode}
                    completedAnswers={askUserCompletedAnswers}
                    readOnly={isAskUserReadOnly}
                    onSubmit={handleAskUserSubmit}
                    onCancel={handleAskUserCancel}
                />
            ),
        });
    }
    if (isPreviewFileDialogOpen && pendingPreviewFileRequest) {
        inlineInteractionItems.push({
            key: `butler-preview-${pendingPreviewFileRequest.request_id}`,
            callId: previewFileCallId,
            messageId: previewFileMessageId,
            content: (
                <PreviewFileCard
                    request={pendingPreviewFileRequest}
                    isOpen={isPreviewFileDialogOpen}
                    onOpenChange={handlePreviewFileOpenChange}
                />
            ),
        });
    }
    const hasInlineInteraction = inlineInteractionItems.length > 0;

    const loadAssistants = useCallback(async () => {
        try {
            const result = await invoke<AssistantListItem[]>("get_assistants");
            setAssistants(result);
        } catch (error) {
            console.error("[ButlerExperimentWindow] Failed to load assistants:", error);
            setAssistants([]);
        }
    }, []);

    const applyMainConversationResult = useCallback((result: ButlerMainLoadResponse) => {
        const nextTasks = sortTasks(result.tasks);
        setLoadError(null);
        setMainConversationId(String(result.conversation.id));
        setMainModelDisplayName(result.model_display_name);
        setTasks(nextTasks);
        setSelectedTaskId((current) => {
            if (current && nextTasks.some((task) => task.task_conversation_id === current)) {
                return current;
            }
            return null;
        });
    }, []);

    const loadMainConversation = useCallback(async (options?: {
        showLoading?: boolean;
        silentError?: boolean;
    }) => {
        const showLoading = options?.showLoading ?? false;
        const silentError = options?.silentError ?? false;
        if (showLoading) {
            setLoadingMain(true);
        }
        try {
            const result = await invoke<ButlerMainLoadResponse>(
                "load_butler_main_conversation"
            );
            applyMainConversationResult(result);
        } catch (error) {
            const message =
                error instanceof Error ? error.message : "无法加载总管家实验窗口";
            console.error("[ButlerExperimentWindow] Failed to load main conversation:", error);
            if (!silentError) {
                setLoadError(message);
            }
            if (!silentError) {
                toast.error(message);
            }
        } finally {
            if (showLoading) {
                setLoadingMain(false);
            }
        }
    }, [applyMainConversationResult]);

    const loadTaskDetail = useCallback(async (taskConversationId: number) => {
        setLoadingTaskDetail(true);
        try {
            const detail = await invoke<ButlerTaskDetailResponse>(
                "get_butler_task_detail",
                {
                    taskConversationId,
                }
            );
            setSelectedTaskDetail(detail);
        } catch (error) {
            console.error("[ButlerExperimentWindow] Failed to load task detail:", error);
            toast.error("加载任务详情失败");
        } finally {
            setLoadingTaskDetail(false);
        }
    }, []);

    const loadFeishuStatus = useCallback(
        async (options?: { silent?: boolean }) => {
            if (!showFeishuStatus) {
                setFeishuStatus(null);
                return;
            }
            const silent = options?.silent ?? false;
            if (!silent) {
                setLoadingFeishuStatus(true);
            }
            try {
                const status = await invoke<FeishuRuntimeStatus>(
                    "get_butler_feishu_runtime_status"
                );
                setFeishuStatus(status);
            } catch (error) {
                console.error(
                    "[ButlerExperimentWindow] Failed to load Feishu runtime status:",
                    error
                );
                if (!silent) {
                    toast.error("加载飞书连接状态失败");
                }
            } finally {
                if (!silent) {
                    setLoadingFeishuStatus(false);
                }
            }
        },
        [showFeishuStatus]
    );

    useEffect(() => {
        void loadAssistants();
        void loadMainConversation({ showLoading: true });
    }, [loadAssistants, loadMainConversation]);

    useEffect(() => {
        if (assistants.length === 0) {
            if (taskAssistantId) {
                setTaskAssistantId("");
            }
            return;
        }
        if (!assistants.some((assistant) => String(assistant.id) === taskAssistantId)) {
            setTaskAssistantId(String(assistants[0].id));
        }
    }, [assistants, taskAssistantId]);

    useEffect(() => {
        if (!selectedTaskId) {
            setSelectedTaskDetail(null);
            return;
        }
        void loadTaskDetail(selectedTaskId);
    }, [loadTaskDetail, selectedTaskId]);

    useEffect(() => {
        const unlisten = listen("feature_config_changed", () => {
            void loadFeatureConfig();
        });
        return () => {
            unlisten.then((fn) => fn()).catch(console.warn);
        };
    }, [loadFeatureConfig]);

    useEffect(() => {
        if (!showFeishuStatus) {
            setFeishuStatus(null);
            setLoadingFeishuStatus(false);
            return;
        }

        void loadFeishuStatus({ silent: true });
        const intervalId = window.setInterval(() => {
            void loadFeishuStatus({ silent: true });
        }, 5000);
        const statusListener = listen<FeishuRuntimeStatus>(
            "butler_feishu_status_changed",
            ({ payload }) => {
                setFeishuStatus(payload);
            }
        );

        return () => {
            window.clearInterval(intervalId);
            statusListener.then((unlisten) => unlisten()).catch(console.warn);
        };
    }, [loadFeishuStatus, showFeishuStatus]);

    useEffect(() => {
        let mounted = true;
        const loadPlugins = async (forceReload = false) => {
            try {
                const plugins = forceReload
                    ? await pluginRuntime.reloadPlugins()
                    : await pluginRuntime.loadPlugins();
                if (mounted) {
                    setPluginList(plugins);
                }
            } catch (error) {
                console.error("[ButlerExperimentWindow] Failed to load plugins:", error);
                if (mounted) {
                    setPluginList([]);
                }
            }
        };

        void loadPlugins();
        const unlistenRegistryChanged = listen("plugin_registry_changed", () => {
            void loadPlugins(true);
        });

        return () => {
            mounted = false;
            unlistenRegistryChanged.then((unlisten) => unlisten()).catch(console.warn);
        };
    }, []);

    useEffect(() => {
        const unlistenHidden = listen("butler-window-hidden", () => {
            setTaskTitle("");
            setTaskGoal("");
            setIsTaskDialogOpen(false);
            setIsTaskDetailDialogOpen(false);
            setSelectedTaskId(null);
            setSelectedTaskDetail(null);
        });
        const focusListener = getCurrentWebviewWindow().onFocusChanged(({ payload }) => {
            if (payload) {
                void loadMainConversation({ silentError: true });
                if (showFeishuStatus) {
                    void loadFeishuStatus({ silent: true });
                }
                if (selectedTaskId && isTaskDetailDialogOpen) {
                    void loadTaskDetail(selectedTaskId);
                }
            }
        });

        return () => {
            unlistenHidden.then((unlisten) => unlisten()).catch(console.warn);
            focusListener.then((unlisten) => unlisten()).catch(console.warn);
        };
    }, [
        isTaskDetailDialogOpen,
        loadFeishuStatus,
        loadMainConversation,
        loadTaskDetail,
        selectedTaskId,
        showFeishuStatus,
    ]);

    useEffect(() => {
        if (!conversationIdNumber) {
            return;
        }

        const taskListeners = [
            listen<ButlerTaskListItem>("butler_task_created", ({ payload }) => {
                if (payload.butler_conversation_id !== conversationIdNumber) {
                    return;
                }
                setTasks((current) => upsertTask(current, payload));
            }),
            listen<ButlerTaskListItem>("butler_task_updated", ({ payload }) => {
                if (payload.butler_conversation_id !== conversationIdNumber) {
                    return;
                }
                setTasks((current) => upsertTask(current, payload));
                if (selectedTaskId === payload.task_conversation_id) {
                    void loadTaskDetail(payload.task_conversation_id);
                }
            }),
            listen<ButlerTaskResultAvailableEvent>(
                "butler_task_result_available",
                ({ payload }) => {
                    if (payload.task.butler_conversation_id !== conversationIdNumber) {
                        return;
                    }
                    setTasks((current) => upsertTask(current, payload.task));
                    if (selectedTaskId === payload.task.task_conversation_id) {
                        void loadTaskDetail(payload.task.task_conversation_id);
                    }
                }
            ),
            listen<ButlerNotificationEvent>("butler_notification_created", ({ payload }) => {
                if (payload.butler_conversation_id !== conversationIdNumber) {
                    return;
                }
                toast.message(payload.title, {
                    description: payload.body,
                });
            }),
        ];

        return () => {
            taskListeners.forEach((listener) => {
                listener.then((unlisten) => unlisten()).catch(console.warn);
            });
        };
    }, [conversationIdNumber, loadTaskDetail, selectedTaskId]);

    const handleResetMainConversation = useCallback(async () => {
        setResettingMainConversation(true);
        try {
            const result = await invoke<ButlerMainLoadResponse>(
                "reset_butler_main_conversation"
            );
            applyMainConversationResult(result);
            setSelectedTaskId(null);
            setSelectedTaskDetail(null);
            setIsTaskDetailDialogOpen(false);
            toast.success("已重开新的总管家主会话");
        } catch (error) {
            console.error("[ButlerExperimentWindow] Failed to reset main conversation:", error);
            toast.error(
                error instanceof Error ? error.message : "重开总管家主会话失败"
            );
        } finally {
            setResettingMainConversation(false);
        }
    }, [applyMainConversationResult]);

    const handleCreateTask = useCallback(async () => {
        if (!mainConversationId) {
            return;
        }
        if (!taskTitle.trim()) {
            toast.error("请输入任务标题");
            return;
        }
        if (!taskGoal.trim()) {
            toast.error("请输入任务目标");
            return;
        }
        if (assistants.length === 0) {
            toast.error("请先创建至少一个可执行任务的助手");
            return;
        }

        setCreatingTask(true);
        try {
            const response = await invoke("spawn_butler_task_conversation", {
                request: {
                    butler_conversation_id: parseInt(mainConversationId, 10),
                    title: taskTitle.trim(),
                    goal: taskGoal.trim(),
                    executor_assistant_id: taskAssistantId
                        ? parseInt(taskAssistantId, 10)
                        : null,
                },
            });
            console.log("[ButlerExperimentWindow] Spawn task response:", response);
            toast.success("任务已派发");
            setTaskTitle("");
            setTaskGoal("");
            setIsTaskDialogOpen(false);
        } catch (error) {
            console.error("[ButlerExperimentWindow] Failed to spawn task:", error);
            toast.error(
                error instanceof Error ? error.message : "派发任务失败"
            );
        } finally {
            setCreatingTask(false);
        }
    }, [mainConversationId, taskAssistantId, taskGoal, taskTitle]);

    const handleOpenTaskConversation = useCallback(async (taskConversationId: number) => {
        const sendSelect = () => {
            emitTo(
                "chat_ui",
                "select_conversation",
                String(taskConversationId)
            ).catch(console.warn);
        };
        once("chat-ui-window-load", sendSelect).catch(console.warn);
        sendSelect();
        await invoke("open_chat_ui_window");
    }, []);

    const handleCancelTask = useCallback(async (taskConversationId: number) => {
        try {
            await invoke("cancel_ai", { conversationId: taskConversationId });
            toast.success("已请求取消任务");
        } catch (error) {
            console.error("[ButlerExperimentWindow] Failed to cancel task:", error);
            toast.error("取消任务失败");
        }
    }, []);

    const handleOpenTaskDetail = useCallback((taskConversationId: number) => {
        setSelectedTaskId(taskConversationId);
        setIsTaskDetailDialogOpen(true);
    }, []);

    const selectedTaskOutput = useMemo(() => {
        const structured = safeParseJson<{ content?: string }>(
            selectedTaskDetail?.result?.structured_output_json
        );
        return (
            structured?.content ||
            selectedTaskDetail?.result?.summary ||
            selectedTaskDetail?.task.last_summary ||
            ""
        );
    }, [selectedTaskDetail]);

    return (
        <AntiLeakageProvider enabled={antiLeakageEnabled}>
            <div
                className="flex h-screen bg-background text-foreground"
                data-aipp-window="butler_experiment"
                data-aipp-slot="window-root"
            >
                <div
                    className="flex-none w-[320px] flex flex-col shadow-lg box-border rounded-r-xl mb-2 mr-2 bg-background overflow-hidden"
                    data-aipp-slot="butler-task-rail"
                >
                    <div
                        className="flex items-start justify-between gap-3 px-5 py-4 border-b border-border"
                        data-aipp-slot="butler-task-rail-header"
                    >
                        <div className="min-w-0">
                            <div className="flex items-center gap-2 text-sm font-semibold">
                                <Bot className="h-4 w-4" />
                                任务台
                                {showFeishuStatus ? (
                                    <Badge
                                        variant={getFeishuStatusVariant(feishuStatus)}
                                        className="gap-1"
                                        title={feishuStatus?.status_text || "正在读取飞书连接状态"}
                                    >
                                        {getFeishuStatusIcon(
                                            feishuStatus,
                                            loadingFeishuStatus
                                        )}
                                        飞书
                                    </Badge>
                                ) : null}
                            </div>
                            <p className="mt-1 text-xs text-muted-foreground">
                                {mainConversationId
                                    ? (mainModelDisplayName || "总管家主会话已就绪")
                                    : (loadingMain
                                        ? "正在准备总管家主会话..."
                                        : "总管家主会话尚未初始化")}
                            </p>
                        </div>
                        <Badge variant="secondary">{tasks.length}</Badge>
                    </div>

                    <div
                        className="flex flex-wrap items-center gap-2 px-4 py-3 border-b border-border"
                        data-aipp-slot="butler-task-rail-actions"
                    >
                        <Button
                            type="button"
                            size="sm"
                            onClick={() => setIsTaskDialogOpen(true)}
                            disabled={!mainConversationId}
                        >
                            <Plus className="mr-2 h-4 w-4" />
                            派发任务
                        </Button>
                        <Button
                            type="button"
                            size="icon"
                            variant="ghost"
                            onClick={() => void loadMainConversation()}
                            aria-label="刷新任务台"
                        >
                            <RefreshCw className="h-4 w-4" />
                        </Button>
                    </div>

                    <ScrollArea className="min-h-0 flex-1" data-aipp-slot="butler-task-list-scroll">
                        <div className="space-y-2 p-3">
                            {tasks.length === 0 ? (
                                <div className="space-y-3 rounded-xl border border-dashed border-border p-4 text-sm text-muted-foreground">
                                    <div>
                                        {mainConversationId ? (
                                            <>
                                                暂无任务。你可以在主会话中调用
                                                <code className="mx-1">
                                                    spawn_task_conversation
                                                </code>
                                                ，或点击上方按钮手动派发。
                                            </>
                                        ) : (
                                            "正在准备总管家任务台..."
                                        )}
                                    </div>
                                    <Button
                                        type="button"
                                        variant="outline"
                                        size="sm"
                                        onClick={() => setIsTaskDialogOpen(true)}
                                        disabled={!mainConversationId}
                                    >
                                        <Plus className="mr-2 h-4 w-4" />
                                        新建第一个任务
                                    </Button>
                                </div>
                            ) : null}
                            {tasks.map((task) => (
                                <button
                                    key={task.task_conversation_id}
                                    type="button"
                                    className={`w-full rounded-xl p-3 text-left transition-colors ${
                                        isTaskDetailDialogOpen &&
                                        selectedTaskId === task.task_conversation_id
                                            ? "bg-primary/8 ring-1 ring-primary"
                                            : "hover:bg-muted/40"
                                    }`}
                                    onClick={() =>
                                        handleOpenTaskDetail(task.task_conversation_id)
                                    }
                                >
                                    <div className="flex items-start justify-between gap-2">
                                        <div className="min-w-0">
                                            <div className="font-medium truncate">
                                                {task.title}
                                            </div>
                                            <div className="text-xs text-muted-foreground mt-1 truncate">
                                                {task.executor_assistant_name}
                                            </div>
                                        </div>
                                        <div className="flex items-center gap-2">
                                            {task.has_pending_permission ? (
                                                <Badge variant="secondary">待审批</Badge>
                                            ) : null}
                                            <Badge variant={getStatusVariant(task.status)}>
                                                {getStatusLabel(task.status)}
                                            </Badge>
                                        </div>
                                    </div>
                                    <div className="mt-2 text-xs text-muted-foreground line-clamp-2">
                                        {task.last_summary || task.goal}
                                    </div>
                                    <div className="mt-2 text-[11px] text-muted-foreground">
                                        更新于 {formatTime(task.updated_time)}
                                    </div>
                                </button>
                            ))}
                        </div>
                    </ScrollArea>
                </div>

                <div
                    className="flex-1 min-w-0 bg-background overflow-hidden rounded-xl m-2 ml-0 shadow-lg"
                    data-aipp-slot="butler-main-content"
                >
                    {loadError ? (
                        <div className="flex h-full items-center justify-center p-6">
                            <div className="max-w-md space-y-4 rounded-xl border border-dashed border-border p-6 text-sm text-muted-foreground">
                                <div className="space-y-1">
                                    <div className="text-base font-semibold text-foreground">
                                        无法打开总管家实验窗口
                                    </div>
                                    <p>{loadError}</p>
                                </div>
                                <Button
                                    type="button"
                                    variant="outline"
                                    onClick={() => void invoke("open_config_window")}
                                >
                                    打开设置
                                </Button>
                            </div>
                        </div>
                    ) : mainConversationId ? (
                        <ConversationUI
                            key={mainConversationId}
                            ref={conversationUIRef}
                            conversationId={mainConversationId}
                            onChangeConversationId={() => undefined}
                            pluginList={pluginList}
                            allowRename={false}
                            allowDelete={false}
                            headerExtraActions={(
                                <Button
                                    type="button"
                                    variant="outline"
                                    size="sm"
                                    onClick={() => void handleResetMainConversation()}
                                    disabled={!mainConversationId || resettingMainConversation}
                                >
                                    {resettingMainConversation ? (
                                        <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                                    ) : (
                                        <RefreshCw className="mr-2 h-4 w-4" />
                                    )}
                                    重开新会话
                                </Button>
                            )}
                            inlineInteractionItems={inlineInteractionItems}
                            inlineInteractionVisible={hasInlineInteraction}
                        />
                    ) : (
                        <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
                            {loadingMain
                                ? "正在准备总管家主会话..."
                                : "总管家主会话尚未初始化"}
                        </div>
                    )}
                </div>

                <OperationPermissionDialog
                    request={pendingRequest}
                    isOpen={isDialogOpen}
                    errorMessage={decisionError}
                    onDecision={handleDecision}
                />
                <AcpPermissionDialog
                    request={pendingAcpRequest}
                    isOpen={isAcpDialogOpen}
                    errorMessage={acpDecisionError}
                    onDecision={handleAcpDecision}
                />
                <Dialog
                    open={isTaskDetailDialogOpen}
                    onOpenChange={(open) => {
                        setIsTaskDetailDialogOpen(open);
                        if (!open) {
                            setSelectedTaskId(null);
                            setSelectedTaskDetail(null);
                        }
                    }}
                >
                    <DialogContent className="max-w-4xl">
                        <DialogHeader>
                            <DialogTitle>任务详情</DialogTitle>
                            <DialogDescription>
                                查看任务状态、目标、摘要与最终输出。
                            </DialogDescription>
                        </DialogHeader>
                        {!selectedTaskId ? (
                            <div className="flex h-48 items-center justify-center text-sm text-muted-foreground">
                                请选择一个任务。
                            </div>
                        ) : loadingTaskDetail && !selectedTaskDetail ? (
                            <div className="flex h-48 items-center justify-center gap-2 text-sm text-muted-foreground">
                                <Loader2 className="h-4 w-4 animate-spin" />
                                正在加载详情...
                            </div>
                        ) : selectedTaskDetail ? (
                            <div className="flex max-h-[75vh] flex-col overflow-hidden">
                                <div className="space-y-3">
                                    <div className="flex items-start justify-between gap-2">
                                        <div>
                                            <div className="font-medium">
                                                {selectedTaskDetail.task.title}
                                            </div>
                                            <div className="mt-1 text-xs text-muted-foreground">
                                                执行助手：
                                                {selectedTaskDetail.task.executor_assistant_name}
                                            </div>
                                        </div>
                                        <Badge
                                            variant={getStatusVariant(
                                                selectedTaskDetail.task.status
                                            )}
                                        >
                                            {getStatusLabel(selectedTaskDetail.task.status)}
                                        </Badge>
                                    </div>
                                    <div className="flex flex-wrap gap-2">
                                        <Button
                                            type="button"
                                            variant="outline"
                                            size="sm"
                                            onClick={() =>
                                                void loadTaskDetail(selectedTaskId)
                                            }
                                        >
                                            <RefreshCw className="mr-2 h-4 w-4" />
                                            刷新
                                        </Button>
                                        <Button
                                            type="button"
                                            variant="outline"
                                            size="sm"
                                            onClick={() =>
                                                void handleOpenTaskConversation(
                                                    selectedTaskDetail.task
                                                        .task_conversation_id
                                                )
                                            }
                                        >
                                            <ExternalLink className="mr-2 h-4 w-4" />
                                            打开任务会话
                                        </Button>
                                        {selectedTaskDetail.task.is_running ? (
                                            <Button
                                                type="button"
                                                variant="outline"
                                                size="sm"
                                                onClick={() =>
                                                    void handleCancelTask(
                                                        selectedTaskDetail.task
                                                            .task_conversation_id
                                                    )
                                                }
                                            >
                                                <PauseCircle className="mr-2 h-4 w-4" />
                                                取消任务
                                            </Button>
                                        ) : null}
                                    </div>
                                </div>

                                <Separator className="my-4" />

                                <ScrollArea className="min-h-0 flex-1 pr-2">
                                    <div className="space-y-4">
                                        <div>
                                            <div className="mb-1 text-xs font-medium text-muted-foreground">
                                                运行状态
                                            </div>
                                            <div className="text-sm">
                                                {selectedTaskDetail.runtime_state.phase}
                                            </div>
                                        </div>
                                        <div>
                                            <div className="mb-1 text-xs font-medium text-muted-foreground">
                                                创建时间
                                            </div>
                                            <div className="text-sm">
                                                {formatTime(
                                                    selectedTaskDetail.task.created_time
                                                )}
                                            </div>
                                        </div>
                                        <div>
                                            <div className="mb-1 text-xs font-medium text-muted-foreground">
                                                完成时间
                                            </div>
                                            <div className="text-sm">
                                                {formatTime(
                                                    selectedTaskDetail.task.finalized_at
                                                )}
                                            </div>
                                        </div>
                                        <div>
                                            <div className="mb-1 text-xs font-medium text-muted-foreground">
                                                任务目标
                                            </div>
                                            <div className="whitespace-pre-wrap rounded-md border bg-muted/20 p-3 text-sm">
                                                {selectedTaskDetail.definition.goal}
                                            </div>
                                        </div>
                                        <div>
                                            <div className="mb-1 text-xs font-medium text-muted-foreground">
                                                结果摘要
                                            </div>
                                            <div className="whitespace-pre-wrap rounded-md border bg-muted/20 p-3 text-sm">
                                                {selectedTaskDetail.result?.summary ||
                                                    selectedTaskDetail.task.last_summary ||
                                                    "暂无摘要"}
                                            </div>
                                        </div>
                                        <div>
                                            <div className="mb-1 text-xs font-medium text-muted-foreground">
                                                最终输出
                                            </div>
                                            <div className="rounded-md border bg-muted/20 p-3 text-sm">
                                                {selectedTaskOutput ? (
                                                    <UnifiedMarkdown>
                                                        {selectedTaskOutput}
                                                    </UnifiedMarkdown>
                                                ) : (
                                                    <span className="text-muted-foreground">
                                                        暂无可展示的最终输出
                                                    </span>
                                                )}
                                            </div>
                                        </div>
                                    </div>
                                </ScrollArea>
                            </div>
                        ) : (
                            <div className="flex h-48 items-center justify-center text-sm text-muted-foreground">
                                暂无任务详情。
                            </div>
                        )}
                    </DialogContent>
                </Dialog>
                <Dialog open={isTaskDialogOpen} onOpenChange={setIsTaskDialogOpen}>
                    <DialogContent>
                        <DialogHeader>
                            <DialogTitle>手动派发任务</DialogTitle>
                            <DialogDescription>
                                填写要交给子任务助手执行的目标、约束和交付要求。
                            </DialogDescription>
                        </DialogHeader>
                        <div className="space-y-4">
                            <div className="space-y-2">
                                <div className="text-sm font-medium">任务标题</div>
                                <Input
                                    placeholder="例如：整理 PRD 并输出执行方案"
                                    value={taskTitle}
                                    onChange={(event) => setTaskTitle(event.target.value)}
                                />
                            </div>
                            <div className="space-y-2">
                                <div className="text-sm font-medium">任务目标</div>
                                <Textarea
                                    placeholder="描述要交给子任务助手执行的目标、约束和产出要求"
                                    value={taskGoal}
                                    onChange={(event) => setTaskGoal(event.target.value)}
                                    rows={6}
                                />
                            </div>
                            <div className="space-y-2">
                                <div className="text-sm font-medium">执行助手</div>
                                <Select
                                    value={taskAssistantId}
                                    onValueChange={setTaskAssistantId}
                                >
                                    <SelectTrigger>
                                        <SelectValue placeholder="选择执行助手" />
                                    </SelectTrigger>
                                    <SelectContent>
                                        {assistants.map((assistant) => (
                                            <SelectItem
                                                key={assistant.id}
                                                value={String(assistant.id)}
                                            >
                                                {assistant.name}
                                            </SelectItem>
                                        ))}
                                    </SelectContent>
                                </Select>
                                {assistants.length === 0 ? (
                                    <p className="text-xs text-muted-foreground">
                                        当前没有可用执行助手，请先去助手配置中创建。
                                    </p>
                                ) : null}
                            </div>
                        </div>
                        <DialogFooter>
                            <Button
                                type="button"
                                variant="outline"
                                onClick={() => setIsTaskDialogOpen(false)}
                            >
                                取消
                            </Button>
                            <Button
                                type="button"
                                onClick={() => void handleCreateTask()}
                                disabled={
                                    creatingTask ||
                                    !mainConversationId ||
                                    assistants.length === 0
                                }
                            >
                                {creatingTask ? (
                                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                                ) : (
                                    <Plus className="mr-2 h-4 w-4" />
                                )}
                                派发任务
                            </Button>
                        </DialogFooter>
                    </DialogContent>
                </Dialog>
            </div>
        </AntiLeakageProvider>
    );
}

export default ButlerExperimentWindow;
