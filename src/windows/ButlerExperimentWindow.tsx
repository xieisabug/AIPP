import { invoke } from "@tauri-apps/api/core";
import { emitTo, listen, once } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { toast } from "sonner";
import {
    Bot,
    Check,
    ChevronDown,
    ChevronRight,
    Clock,
    ExternalLink,
    Loader2,
    PauseCircle,
    Plus,
    RefreshCw,
    Settings,
    X,
} from "lucide-react";

import ConversationUI, {
    ConversationUIRef,
    type InlineInteractionItem,
    type PreviewFileContextSelection,
} from "@/components/ConversationUI";
import IconButton from "@/components/IconButton";
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
import { Checkbox } from "@/components/ui/checkbox";
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
import { FolderPicker } from "@/components/config/FolderPicker";
import { ExperimentalConfigForm } from "@/components/config/feature/forms/ExperimentalConfigForm";
import {
    buildExperimentalConfigFormValues,
    EXPERIMENTAL_CONFIG_DEFAULT_VALUES,
    ExperimentalConfigFormState,
    saveExperimentalConfigValues,
} from "@/components/config/feature/forms/experimentalConfigShared";
import { ConversationStatsDialog } from "@/components/token-statistics";
import { ButlerOnboardingWizard } from "@/components/butler/ButlerOnboardingWizard";
import { buildButlerWorkspaceConfig } from "@/components/butler/butlerWorkspaceConfig";
import { AssistantListItem } from "@/data/Assistant";
import {
    ButlerMainLoadResponse,
    ButlerNotificationEvent,
    ButlerTaskDetailResponse,
    ButlerTaskListItem,
    ButlerTaskResultAvailableEvent,
    PaginatedButlerTasksResponse,
} from "@/data/Butler";
import { ScannedSkill } from "@/data/Skill";
import { useAskUserQuestion, usePreviewFile } from "@/hooks/useInlineInteraction";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import { useAppShortcuts } from "@/hooks/useAppShortcuts";
import { useTheme } from "@/hooks/useTheme";
import { useAcpPermission, useOperationPermission } from "@/hooks/useOperationPermission";
import { useAcpElicitation } from "@/hooks/useAcpElicitation";
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

function normalizeTaskTrustedPaths(paths: string[]): string[] {
    const seen = new Set<string>();
    const normalized: string[] = [];
    for (const path of paths) {
        const value = path.trim();
        if (!value) {
            continue;
        }
        const key = value.toLowerCase();
        if (seen.has(key)) {
            continue;
        }
        seen.add(key);
        normalized.push(value);
    }
    return normalized;
}

function normalizeIdentifiers(values: string[]): string[] {
    const seen = new Set<string>();
    const normalized: string[] = [];
    for (const value of values) {
        const trimmed = value.trim();
        if (!trimmed || seen.has(trimmed)) {
            continue;
        }
        seen.add(trimmed);
        normalized.push(trimmed);
    }
    return normalized;
}

interface ButlerScheduledTask {
    id: number;
    name: string;
    isEnabled: boolean;
    scheduleType: "once" | "interval";
    intervalValue?: number | null;
    intervalUnit?: string | null;
    startTime?: string | null;
    nextRunAt?: string | null;
    lastRunAt?: string | null;
    taskPrompt: string;
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

const TASK_PAGE_SIZE = 20;

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

interface ButlerMainConversationEvent {
    conversation_id: number;
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
    const {
        featureConfig,
        getConfigValue,
        loadFeatureConfig,
        loading: loadingFeatureConfig,
        saveFeatureConfig,
    } = useFeatureConfig();
    const antiLeakageEnabled = getConfigValue("anti_leakage", "enabled") === "true";
    const butlerExperimentEnabled =
        getConfigValue("experimental", "butler_experiment_enabled") === "true";
    const butlerDisplayName = (
        getConfigValue("experimental", "butler_display_name", "总管家").trim() || "总管家"
    );
    const feishuEnabled = getConfigValue("experimental", "butler_feishu_enabled") === "true";
    const showFeishuStatus = butlerExperimentEnabled && feishuEnabled;

    const conversationUIRef = useRef<ConversationUIRef>(null);
    const mainLoadRequestRef = useRef(0);
    const latestTaskDetailRequestRef = useRef(0);
    const selectedTaskIdRef = useRef<number | null>(null);
    const isTaskDetailDialogOpenRef = useRef(false);
    const resettingMainConversationRef = useRef(false);
    const mainConversationIdRef = useRef("");
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
    const [isStatsDialogOpen, setIsStatsDialogOpen] = useState(false);
    const [isSettingsDialogOpen, setIsSettingsDialogOpen] = useState(false);
    const [isTaskDialogOpen, setIsTaskDialogOpen] = useState(false);
    const [isTaskDetailDialogOpen, setIsTaskDetailDialogOpen] = useState(false);
    const [taskTitle, setTaskTitle] = useState("");
    const [taskGoal, setTaskGoal] = useState("");
    const [taskAssistantId, setTaskAssistantId] = useState<string>("");
    const [taskTemporaryPathInput, setTaskTemporaryPathInput] = useState("");
    const [taskTemporaryTrustedPaths, setTaskTemporaryTrustedPaths] = useState<string[]>([]);
    const [taskTemporarySkillIdentifiers, setTaskTemporarySkillIdentifiers] = useState<string[]>([]);
    const [taskSkillQuery, setTaskSkillQuery] = useState("");
    const [availableSkills, setAvailableSkills] = useState<ScannedSkill[]>([]);
    const [loadingAvailableSkills, setLoadingAvailableSkills] = useState(false);
    const [loadError, setLoadError] = useState<string | null>(null);
    const [feishuStatus, setFeishuStatus] = useState<FeishuRuntimeStatus | null>(null);
    const [isMainConversationFeishuBound, setIsMainConversationFeishuBound] =
        useState(false);
    const [scheduledTasks, setScheduledTasks] = useState<ButlerScheduledTask[]>([]);
    const [isScheduledTasksExpanded, setIsScheduledTasksExpanded] = useState(false);
    const [totalTasks, setTotalTasks] = useState(0);
    const [loadingMoreTasks, setLoadingMoreTasks] = useState(false);
    const [hasMoreTasks, setHasMoreTasks] = useState(false);
    const [isOnboardingOpen, setIsOnboardingOpen] = useState(false);
    const onboardingAutoTriggeredRef = useRef(false);
    const butlerSettingsForm = useForm<ExperimentalConfigFormState>({
        defaultValues: { ...EXPERIMENTAL_CONFIG_DEFAULT_VALUES },
    });

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

    const filteredAvailableSkills = useMemo(() => {
        const query = taskSkillQuery.trim().toLowerCase();
        if (!query) {
            return availableSkills;
        }
        return availableSkills.filter((skill) => {
            const displayName = skill.display_name.toLowerCase();
            const identifier = skill.identifier.toLowerCase();
            return displayName.includes(query) || identifier.includes(query);
        });
    }, [availableSkills, taskSkillQuery]);

    const availableSkillNameByIdentifier = useMemo(
        () =>
            new Map(
                availableSkills.map((skill) => [skill.identifier, skill.display_name] as const)
            ),
        [availableSkills]
    );

    const {
        pendingRequest,
        isDialogOpen,
        decisionError,
        isSubmitting,
        handleDecision,
    } = useOperationPermission({
        conversationId: conversationIdNumber,
        conversationIds: permissionConversationIds,
    });
    const {
        pendingRequest: pendingAcpRequest,
        isDialogOpen: isAcpDialogOpen,
        decisionError: acpDecisionError,
        isSubmitting: isAcpSubmitting,
        handleDecision: handleAcpDecision,
    } = useAcpPermission({
        conversationId: conversationIdNumber,
        conversationIds: permissionConversationIds,
    });
    const {
        questionRequest: elicitationQuestionRequest,
        decisionError: elicitationDecisionError,
        handleSubmit: handleElicitationSubmit,
        handleCancel: handleElicitationCancel,
    } = useAcpElicitation({
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
        reopenPersistedPreview: reopenPersistedPreviewFile,
    } = usePreviewFile({
        conversationId: conversationIdNumber,
    });

    const handlePreviewFileContextSelection = useCallback((selection: PreviewFileContextSelection) => {
        if (selection.messageId) {
            conversationUIRef.current?.scrollToMessage(selection.messageId);
        }
        reopenPersistedPreviewFile(selection.callId);
    }, [reopenPersistedPreviewFile]);

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
    if (elicitationQuestionRequest) {
        inlineInteractionItems.push({
            key: `butler-acp-elicitation-${elicitationQuestionRequest.request_id}`,
            callId: null,
            messageId: null,
            content: (
                <AskUserQuestionCard
                    request={elicitationQuestionRequest}
                    isOpen
                    viewMode="questionnaire"
                    errorMessage={elicitationDecisionError}
                    onSubmit={handleElicitationSubmit}
                    onCancel={handleElicitationCancel}
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

    const loadAvailableSkills = useCallback(async () => {
        try {
            setLoadingAvailableSkills(true);
            const result = await invoke<ScannedSkill[]>("scan_skills");
            result.sort((left, right) =>
                left.display_name.localeCompare(right.display_name, "zh-CN")
            );
            setAvailableSkills(result);
        } catch (error) {
            console.error("[ButlerExperimentWindow] Failed to load skills:", error);
            setAvailableSkills([]);
            toast.error("加载临时 Skills 列表失败");
        } finally {
            setLoadingAvailableSkills(false);
        }
    }, []);

    const resetTaskDialogState = useCallback(() => {
        setTaskTitle("");
        setTaskGoal("");
        setTaskAssistantId("");
        setTaskTemporaryPathInput("");
        setTaskTemporaryTrustedPaths([]);
        setTaskTemporarySkillIdentifiers([]);
        setTaskSkillQuery("");
    }, []);

    const handleTaskDialogOpenChange = useCallback(
        (open: boolean) => {
            setIsTaskDialogOpen(open);
            if (!open) {
                resetTaskDialogState();
            }
        },
        [resetTaskDialogState]
    );

    useEffect(() => {
        if (!isTaskDialogOpen) {
            return;
        }
        void loadAvailableSkills();
    }, [isTaskDialogOpen, loadAvailableSkills]);

    const applyMainConversationResult = useCallback((result: ButlerMainLoadResponse) => {
        const nextTasks = sortTasks(result.tasks);
        setLoadError(null);
        setMainConversationId(String(result.conversation.id));
        setMainModelDisplayName(result.model_display_name);
        setTasks(nextTasks);
        setTotalTasks(result.total_tasks);
        setHasMoreTasks(nextTasks.length < result.total_tasks);
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
        reconcile?: boolean;
    }) => {
        const requestId = ++mainLoadRequestRef.current;
        const showLoading = options?.showLoading ?? false;
        const silentError = options?.silentError ?? false;
        const reconcile = options?.reconcile ?? false;
        if (showLoading) {
            setLoadingMain(true);
        }
        try {
            const result = await invoke<ButlerMainLoadResponse>(
                "load_butler_main_conversation",
                { reconcile }
            );
            if (requestId !== mainLoadRequestRef.current) {
                return;
            }
            applyMainConversationResult(result);
        } catch (error) {
            if (requestId !== mainLoadRequestRef.current) {
                return;
            }
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
            if (showLoading && requestId === mainLoadRequestRef.current) {
                setLoadingMain(false);
            }
        }
    }, [applyMainConversationResult]);

    const loadTaskDetail = useCallback(async (taskConversationId: number) => {
        const requestId = ++latestTaskDetailRequestRef.current;
        setLoadingTaskDetail(true);
        try {
            const detail = await invoke<ButlerTaskDetailResponse>(
                "get_butler_task_detail",
                {
                    taskConversationId,
                }
            );
            if (requestId !== latestTaskDetailRequestRef.current) {
                return;
            }
            setSelectedTaskDetail(detail);
        } catch (error) {
            if (requestId !== latestTaskDetailRequestRef.current) {
                return;
            }
            console.error("[ButlerExperimentWindow] Failed to load task detail:", error);
            toast.error("加载任务详情失败");
        } finally {
            if (requestId === latestTaskDetailRequestRef.current) {
                setLoadingTaskDetail(false);
            }
        }
    }, []);

    const closeTaskDetail = useCallback(() => {
        latestTaskDetailRequestRef.current += 1;
        setIsTaskDetailDialogOpen(false);
        setSelectedTaskId(null);
        setSelectedTaskDetail(null);
        setLoadingTaskDetail(false);
    }, []);

    useEffect(() => {
        selectedTaskIdRef.current = selectedTaskId;
    }, [selectedTaskId]);

    useEffect(() => {
        resettingMainConversationRef.current = resettingMainConversation;
    }, [resettingMainConversation]);

    useEffect(() => {
        mainConversationIdRef.current = mainConversationId;
    }, [mainConversationId]);

    useEffect(() => {
        isTaskDetailDialogOpenRef.current = isTaskDetailDialogOpen;
    }, [isTaskDetailDialogOpen]);

    useEffect(() => {
        if (!isTaskDetailDialogOpen) {
            return;
        }

        const handleKeyDown = (event: KeyboardEvent) => {
            if (event.key === "Escape") {
                event.preventDefault();
                closeTaskDetail();
            }
        };

        window.addEventListener("keydown", handleKeyDown);
        return () => {
            window.removeEventListener("keydown", handleKeyDown);
        };
    }, [closeTaskDetail, isTaskDetailDialogOpen]);

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

    const loadMainConversationFeishuBinding = useCallback(
        async (conversationId: number, options?: { silent?: boolean }) => {
            if (!showFeishuStatus) {
                setIsMainConversationFeishuBound(false);
                return;
            }

            const silent = options?.silent ?? false;
            try {
                const isBound = await invoke<boolean>("conversation_has_feishu_target", {
                    conversationId,
                    conversation_id: conversationId,
                });
                setIsMainConversationFeishuBound(isBound);
            } catch (error) {
                console.error(
                    "[ButlerExperimentWindow] Failed to load Feishu binding state:",
                    error
                );
                setIsMainConversationFeishuBound(false);
                if (!silent) {
                    toast.error("加载总管家飞书绑定状态失败");
                }
            }
        },
        [showFeishuStatus]
    );

    const loadButlerScheduledTasks = useCallback(async () => {
        if (!mainConversationId) return;
        try {
            const result = await invoke<ButlerScheduledTask[]>("list_butler_scheduled_tasks", {
                butlerConversationId: parseInt(mainConversationId, 10),
            });
            setScheduledTasks(result);
        } catch (error) {
            console.error("[ButlerExperimentWindow] Failed to load scheduled tasks:", error);
        }
    }, [mainConversationId]);

    const loadMoreTasks = useCallback(async () => {
        if (loadingMoreTasks || !hasMoreTasks || !mainConversationId) {
            return;
        }
        setLoadingMoreTasks(true);
        try {
            const result = await invoke<PaginatedButlerTasksResponse>(
                "list_butler_tasks_paginated",
                {
                    butlerConversationId: parseInt(mainConversationId, 10),
                    limit: TASK_PAGE_SIZE,
                    offset: tasks.length,
                }
            );
            setTasks((current) => {
                const existingIds = new Set(
                    current.map((t) => t.task_conversation_id)
                );
                const newTasks = result.tasks.filter(
                    (t) => !existingIds.has(t.task_conversation_id)
                );
                return sortTasks([...current, ...newTasks]);
            });
            setTotalTasks(result.total);
            setHasMoreTasks(tasks.length + result.tasks.length < result.total);
        } catch (error) {
            console.error("[ButlerExperimentWindow] Failed to load more tasks:", error);
        } finally {
            setLoadingMoreTasks(false);
        }
    }, [loadingMoreTasks, hasMoreTasks, mainConversationId, tasks.length]);

    const handleTaskListScroll = useCallback(
        (e: React.UIEvent<HTMLDivElement>) => {
            const { scrollTop, scrollHeight, clientHeight } = e.currentTarget;
            if (scrollHeight - scrollTop - clientHeight < 100) {
                void loadMoreTasks();
            }
        },
        [loadMoreTasks]
    );

    useEffect(() => {
        void loadAssistants();
        void loadMainConversation({ showLoading: true });
    }, [loadAssistants, loadMainConversation]);

    useEffect(() => {
        void loadButlerScheduledTasks();
    }, [loadButlerScheduledTasks]);

    useEffect(() => {
        if (conversationIdNumber === undefined) {
            return;
        }

        void loadButlerScheduledTasks();
        const intervalId = window.setInterval(() => {
            void loadButlerScheduledTasks();
        }, 30000);
        const listeners = [
            listen("scheduled_task_run_created", () => {
                void loadButlerScheduledTasks();
            }),
            listen("scheduled_task_run_updated", () => {
                void loadButlerScheduledTasks();
            }),
        ];

        return () => {
            window.clearInterval(intervalId);
            listeners.forEach((listener) => {
                listener.then((unlisten) => unlisten()).catch(console.warn);
            });
        };
    }, [conversationIdNumber, loadButlerScheduledTasks]);

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
            setIsMainConversationFeishuBound(false);
            return;
        }

        if (conversationIdNumber === undefined) {
            setIsMainConversationFeishuBound(false);
        }

        void loadFeishuStatus({ silent: true });
        if (conversationIdNumber !== undefined) {
            void loadMainConversationFeishuBinding(conversationIdNumber, {
                silent: true,
            });
        }
        const intervalId = window.setInterval(() => {
            void loadFeishuStatus({ silent: true });
            if (conversationIdNumber !== undefined) {
                void loadMainConversationFeishuBinding(conversationIdNumber, {
                    silent: true,
                });
            }
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
    }, [
        conversationIdNumber,
        loadFeishuStatus,
        loadMainConversationFeishuBinding,
        showFeishuStatus,
    ]);

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
            setIsStatsDialogOpen(false);
            setIsTaskDialogOpen(false);
            closeTaskDetail();
        });

        return () => {
            unlistenHidden.then((unlisten) => unlisten()).catch(console.warn);
        };
    }, [closeTaskDetail]);

    useEffect(() => {
        const unlistenReset = listen<ButlerMainConversationEvent>(
            "butler_main_reset",
            ({ payload }) => {
                const nextConversationId = String(payload.conversation_id);
                if (resettingMainConversationRef.current) {
                    return;
                }
                if (nextConversationId === mainConversationIdRef.current) {
                    return;
                }
                closeTaskDetail();
                void loadMainConversation();
            }
        );

        return () => {
            unlistenReset.then((unlisten) => unlisten()).catch(console.warn);
        };
    }, [closeTaskDetail, loadMainConversation]);

    useEffect(() => {
        if (conversationIdNumber === undefined) {
            return;
        }

        const intervalId = window.setInterval(() => {
            if (
                selectedTaskIdRef.current &&
                isTaskDetailDialogOpenRef.current
            ) {
                void loadTaskDetail(selectedTaskIdRef.current);
            }
        }, 1000);

        return () => {
            window.clearInterval(intervalId);
        };
    }, [conversationIdNumber, loadTaskDetail]);

    useEffect(() => {
        if (!conversationIdNumber) {
            return;
        }

        const taskListeners = [
            listen<ButlerTaskListItem>("butler_task_created", ({ payload }) => {
                if (payload.butler_conversation_id !== conversationIdNumber) {
                    return;
                }
                setTasks((current) => {
                    const isNew = !current.some(
                        (t) => t.task_conversation_id === payload.task_conversation_id
                    );
                    if (isNew) {
                        setTotalTasks((prev) => prev + 1);
                    }
                    return upsertTask(current, payload);
                });
            }),
            listen<ButlerTaskListItem>("butler_task_updated", ({ payload }) => {
                if (payload.butler_conversation_id !== conversationIdNumber) {
                    return;
                }
                setTasks((current) => upsertTask(current, payload));
                if (selectedTaskIdRef.current === payload.task_conversation_id) {
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
                    if (selectedTaskIdRef.current === payload.task.task_conversation_id) {
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
    }, [conversationIdNumber, loadTaskDetail]);

    const handleResetMainConversation = useCallback(async () => {
        resettingMainConversationRef.current = true;
        setResettingMainConversation(true);
        try {
            const result = await invoke<ButlerMainLoadResponse>(
                "reset_butler_main_conversation"
            );
            applyMainConversationResult(result);
            closeTaskDetail();
            toast.success("已重开新的总管家主会话");
        } catch (error) {
            console.error("[ButlerExperimentWindow] Failed to reset main conversation:", error);
            toast.error(
                error instanceof Error ? error.message : "重开总管家主会话失败"
            );
        } finally {
            resettingMainConversationRef.current = false;
            setResettingMainConversation(false);
        }
    }, [applyMainConversationResult, closeTaskDetail]);

    const handleOpenSettings = useCallback(() => {
        loadFeatureConfig()
            .then((latestConfig) => {
                butlerSettingsForm.reset(buildExperimentalConfigFormValues(latestConfig));
                setIsSettingsDialogOpen(true);
            })
            .catch((error) => {
                console.error("[ButlerExperimentWindow] Failed to load settings:", error);
                toast.error("加载总管家设置失败");
            });
    }, [butlerSettingsForm, loadFeatureConfig]);

    const handleSaveButlerSettings = useCallback(async () => {
        await saveExperimentalConfigValues(saveFeatureConfig, butlerSettingsForm.getValues());
    }, [butlerSettingsForm, saveFeatureConfig]);

    const handleToggleSidebar = useCallback(() => {
        conversationUIRef.current?.toggleSidebar();
    }, []);

    const handleOpenSidebarWindow = useCallback(() => {
        conversationUIRef.current?.openSidebarWindow();
    }, []);

    const handleOpenStats = useCallback(() => {
        if (!mainConversationId) {
            return;
        }
        setIsStatsDialogOpen(true);
    }, [mainConversationId]);

    const handleAddTemporaryTrustedPath = useCallback(() => {
        const nextPath = taskTemporaryPathInput.trim();
        if (!nextPath) {
            return;
        }
        setTaskTemporaryTrustedPaths((current) =>
            normalizeTaskTrustedPaths([...current, nextPath])
        );
        setTaskTemporaryPathInput("");
    }, [taskTemporaryPathInput]);

    const handleRemoveTemporaryTrustedPath = useCallback((path: string) => {
        setTaskTemporaryTrustedPaths((current) =>
            current.filter((item) => item !== path)
        );
    }, []);

    const handleToggleTemporarySkill = useCallback((identifier: string, checked: boolean) => {
        setTaskTemporarySkillIdentifiers((current) =>
            checked
                ? normalizeIdentifiers([...current, identifier])
                : current.filter((item) => item !== identifier)
        );
    }, []);

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
                    temporary_trusted_paths: taskTemporaryTrustedPaths,
                    temporary_skill_identifiers: taskTemporarySkillIdentifiers,
                },
            });
            console.log("[ButlerExperimentWindow] Spawn task response:", response);
            toast.success("任务已派发");
            handleTaskDialogOpenChange(false);
        } catch (error) {
            console.error("[ButlerExperimentWindow] Failed to spawn task:", error);
            toast.error(
                error instanceof Error ? error.message : "派发任务失败"
            );
        } finally {
            setCreatingTask(false);
        }
    }, [
        handleTaskDialogOpenChange,
        mainConversationId,
        taskAssistantId,
        taskGoal,
        taskTemporarySkillIdentifiers,
        taskTemporaryTrustedPaths,
        taskTitle,
    ]);

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
        setSelectedTaskDetail(null);
        setLoadingTaskDetail(true);
        setIsTaskDetailDialogOpen(true);
        void loadTaskDetail(taskConversationId);
    }, [loadTaskDetail]);

    useEffect(() => {
        if (loadingFeatureConfig) {
            return;
        }
        butlerSettingsForm.reset(buildExperimentalConfigFormValues(featureConfig));
    }, [butlerSettingsForm, featureConfig, loadingFeatureConfig]);

    const butlerWorkspaceConfig = useMemo(
        () =>
            buildButlerWorkspaceConfig({
                mainWorkspacePath:
                    getConfigValue("experimental", "butler_main_workspace_path") || "",
                mainWorkspaceDescription:
                    getConfigValue("experimental", "butler_main_workspace_description") || "",
                trustedWorkspacesRaw:
                    getConfigValue("experimental", "butler_trusted_workspaces") || "",
            }),
        [getConfigValue]
    );

    // Auto-trigger onboarding when required Butler config is incomplete
    const butlerModelId = getConfigValue("experimental", "butler_model_id") || "";
    useEffect(() => {
        if (loadingFeatureConfig || onboardingAutoTriggeredRef.current) {
            return;
        }
        if (
            butlerExperimentEnabled
            && (!butlerModelId || !butlerWorkspaceConfig.mainWorkspace?.path)
        ) {
            onboardingAutoTriggeredRef.current = true;
            setIsOnboardingOpen(true);
        }
    }, [butlerExperimentEnabled, butlerModelId, butlerWorkspaceConfig.mainWorkspace?.path, loadingFeatureConfig]);

    const handleOnboardingComplete = useCallback(() => {
        void loadFeatureConfig();
    }, [loadFeatureConfig]);

    const existingMainWorkspace = butlerWorkspaceConfig.mainWorkspace;
    const existingTrustedWorkspaces = butlerWorkspaceConfig.trustedWorkspaces;

    useAppShortcuts("butler", {
        new: () => {
            if (!mainConversationId || resettingMainConversation) {
                return;
            }
            void handleResetMainConversation();
        },
        stats: handleOpenStats,
        settings: handleOpenSettings,
        toggle_sidebar: handleToggleSidebar,
        open_sidebar_window: handleOpenSidebarWindow,
    });

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

    const selectedTaskSummary = selectedTaskDetail?.result?.summary ||
        selectedTaskDetail?.task.last_summary ||
        "";

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
                            </div>
                            {mainConversationId || showFeishuStatus ? (
                                <div className="mt-2 flex flex-wrap items-center gap-2">
                                    {mainModelDisplayName ? (
                                        <Badge variant="outline" className="max-w-full">
                                            <span className="truncate">
                                                模型：{mainModelDisplayName}
                                            </span>
                                        </Badge>
                                    ) : null}
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
                                    {!mainModelDisplayName && !showFeishuStatus ? (
                                        <span className="text-xs text-muted-foreground">
                                            总管家主会话已就绪
                                        </span>
                                    ) : null}
                                </div>
                            ) : (
                                <p className="mt-1 text-xs text-muted-foreground">
                                    {loadingMain
                                        ? "正在准备总管家主会话..."
                                        : "总管家主会话尚未初始化"}
                                </p>
                            )}
                        </div>
                        <Badge variant="secondary">{totalTasks}</Badge>
                    </div>

                    <div
                        className="flex flex-wrap items-center gap-2 px-4 py-3 border-b border-border"
                        data-aipp-slot="butler-task-rail-actions"
                    >
                        <IconButton
                            icon={<Plus className="h-4 w-4 text-icon" />}
                            onClick={() => setIsTaskDialogOpen(true)}
                            disabled={!mainConversationId}
                            border
                            title="派发任务"
                            dataAippSlot="butler-task-create"
                        />
                        <IconButton
                            icon={<RefreshCw className="h-4 w-4 text-icon" />}
                            onClick={() => void loadMainConversation({ reconcile: true })}
                            border
                            title="刷新任务台"
                            dataAippSlot="butler-task-refresh"
                        />
                    </div>

                    <div className="min-h-0 flex-1 overflow-y-auto" data-aipp-slot="butler-task-list-scroll" onScroll={handleTaskListScroll}>
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
                                    className={`w-full rounded-xl p-3 text-left transition-colors ${isTaskDetailDialogOpen &&
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
                                        {task.is_finalized
                                            ? `完成于 ${formatTime(task.finalized_at)}`
                                            : "进行中"}
                                    </div>
                                </button>
                            ))}
                            {loadingMoreTasks && (
                                <div className="flex items-center justify-center py-3">
                                    <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                                    <span className="ml-2 text-xs text-muted-foreground">加载更多…</span>
                                </div>
                            )}
                        </div>
                    </div>

                    {scheduledTasks.length > 0 && (
                        <div className="border-t border-border" data-aipp-slot="butler-scheduled-tasks">
                            <button
                                type="button"
                                className="flex w-full items-center gap-2 px-4 py-2.5 text-xs font-medium text-muted-foreground hover:bg-muted/40 transition-colors"
                                onClick={() => setIsScheduledTasksExpanded(prev => !prev)}
                            >
                                {isScheduledTasksExpanded ? (
                                    <ChevronDown className="h-3.5 w-3.5" />
                                ) : (
                                    <ChevronRight className="h-3.5 w-3.5" />
                                )}
                                <Clock className="h-3.5 w-3.5" />
                                定时任务
                                <Badge variant="secondary" className="ml-auto text-[10px] px-1.5 py-0">
                                    {scheduledTasks.length}
                                </Badge>
                            </button>
                            {isScheduledTasksExpanded && (
                                <div className="px-3 pb-3 space-y-1.5 max-h-[200px] overflow-y-auto">
                                    {scheduledTasks.map((st) => (
                                        <div
                                            key={st.id}
                                            className="rounded-lg border border-border/50 px-3 py-2 text-xs"
                                        >
                                            <div className="flex items-center justify-between gap-2">
                                                <span className="font-medium truncate">{st.name}</span>
                                                <Badge
                                                    variant={st.isEnabled ? "default" : "secondary"}
                                                    className="text-[10px] px-1.5 py-0"
                                                >
                                                    {st.isEnabled ? "运行中" : "已暂停"}
                                                </Badge>
                                            </div>
                                            <div className="mt-1 text-muted-foreground truncate">
                                                {st.taskPrompt.length > 60
                                                    ? st.taskPrompt.slice(0, 60) + "…"
                                                    : st.taskPrompt}
                                            </div>
                                            {st.nextRunAt && (
                                                <div className="mt-1 text-[11px] text-muted-foreground">
                                                    下次执行：{formatTime(st.nextRunAt)}
                                                </div>
                                            )}
                                        </div>
                                    ))}
                                </div>
                            )}
                        </div>
                    )}
                </div>

                <div
                    className="flex flex-1 min-w-0 flex-col overflow-hidden rounded-xl bg-background shadow-lg m-2 ml-0"
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
                                    onClick={handleOpenSettings}
                                >
                                    打开设置
                                </Button>
                            </div>
                        </div>
                    ) : mainConversationId ? (
                        <>
                            <div
                                className="flex flex-none items-start justify-between gap-3 border-b border-border px-6 py-4"
                                data-aipp-slot="butler-main-header"
                            >
                                <div className="min-w-0">
                                    <div className="text-base font-semibold">
                                        {butlerDisplayName}
                                    </div>
                                </div>
                                <div className="flex items-center gap-2">
                                    <ConversationStatsDialog
                                        conversationId={mainConversationId}
                                        externalOpen={isStatsDialogOpen}
                                        onExternalOpenChange={setIsStatsDialogOpen}
                                    />
                                    <IconButton
                                        icon={<Settings className="h-4 w-4 text-icon" />}
                                        onClick={handleOpenSettings}
                                        border
                                        title="总管家设置"
                                        dataAippSlot="butler-main-open-settings"
                                    />
                                    <IconButton
                                        icon={
                                            resettingMainConversation ? (
                                                <Loader2 className="h-4 w-4 animate-spin text-icon" />
                                            ) : (
                                                <RefreshCw className="h-4 w-4 text-icon" />
                                            )
                                        }
                                        onClick={() => void handleResetMainConversation()}
                                        disabled={!mainConversationId || resettingMainConversation}
                                        border
                                        title="重开新会话"
                                        dataAippSlot="butler-main-reset-conversation"
                                    />
                                </div>
                            </div>
                            <div className="min-h-0 flex-1">
                                <ConversationUI
                                    key={mainConversationId}
                                    ref={conversationUIRef}
                                    conversationId={mainConversationId}
                                    onChangeConversationId={() => undefined}
                                    pluginList={pluginList}
                                    hideHeader
                                    allowRename={false}
                                    allowDelete={false}
                                    inlineInteractionItems={inlineInteractionItems}
                                    inlineInteractionVisible={hasInlineInteraction}
                                    allowFeishuDebugResend={
                                        showFeishuStatus && isMainConversationFeishuBound
                                    }
                                    virtualizeMessages
                                    virtualizedListEngine="virtuoso"
                                    windowLabel="butler_experiment"
                                    busySendBehavior="interrupt"
                                    onPreviewFileContextClick={handlePreviewFileContextSelection}
                                />
                            </div>
                        </>
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
                    isSubmitting={isSubmitting}
                    errorMessage={decisionError}
                    onDecision={handleDecision}
                />
                <AcpPermissionDialog
                    request={pendingAcpRequest}
                    isOpen={isAcpDialogOpen}
                    isSubmitting={isAcpSubmitting}
                    errorMessage={acpDecisionError}
                    onDecision={handleAcpDecision}
                />
                <Dialog
                    open={isSettingsDialogOpen}
                    onOpenChange={setIsSettingsDialogOpen}
                >
                    <DialogContent className="flex max-h-[90vh] w-[50vw] min-w-[50vw] max-w-none flex-col overflow-hidden p-0">
                        <DialogHeader className="border-b px-6 py-4">
                            <DialogTitle>总管家实验设置</DialogTitle>
                            <DialogDescription>
                                在当前工作台内调整 Butler 相关实验配置，包括模型、上下文压缩、可信工作区和飞书接入。
                            </DialogDescription>
                        </DialogHeader>
                        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
                            <ExperimentalConfigForm
                                form={butlerSettingsForm}
                                onSave={handleSaveButlerSettings}
                                scope="butler"
                                saveFeatureConfig={saveFeatureConfig}
                                onConfigRefresh={handleOnboardingComplete}
                            />
                        </div>
                    </DialogContent>
                </Dialog>
                <ButlerOnboardingWizard
                    open={isOnboardingOpen}
                    onOpenChange={setIsOnboardingOpen}
                    existingModelId={butlerModelId}
                    existingDisplayName={butlerDisplayName}
                    existingTrustAll={getConfigValue("experimental", "butler_trust_all_workspaces") === "true"}
                    existingMainWorkspace={existingMainWorkspace}
                    existingTrustedWorkspaces={existingTrustedWorkspaces}
                    existingFeishuEnabled={feishuEnabled}
                    existingFeishuAppId={getConfigValue("experimental", "butler_feishu_app_id") || ""}
                    existingFeishuBaseUrl={getConfigValue("experimental", "butler_feishu_base_url") || "https://open.feishu.cn"}
                    initialValues={buildExperimentalConfigFormValues(featureConfig)}
                    saveFeatureConfig={saveFeatureConfig}
                    onComplete={handleOnboardingComplete}
                />
                {isTaskDetailDialogOpen ? (
                    <div
                        className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
                        onClick={closeTaskDetail}
                    >
                        <div
                            role="dialog"
                            aria-modal="true"
                            aria-labelledby="butler-task-detail-title"
                            className="flex h-[85vh] w-full max-w-4xl flex-col overflow-hidden rounded-lg border bg-background shadow-lg"
                            onClick={(event) => event.stopPropagation()}
                        >
                            <div className="flex items-start justify-between gap-4 border-b px-6 py-4">
                                <div>
                                    <div
                                        id="butler-task-detail-title"
                                        className="text-lg font-semibold"
                                    >
                                        任务详情
                                    </div>
                                    <p className="text-sm text-muted-foreground">
                                        查看任务状态、目标与结果摘要。
                                    </p>
                                </div>
                                <button
                                    type="button"
                                    className="rounded-sm p-1 text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                                    onClick={closeTaskDetail}
                                    aria-label="关闭任务详情"
                                >
                                    <X className="h-4 w-4" />
                                </button>
                            </div>

                            <div className="flex min-h-0 flex-1 flex-col px-6 py-4">
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
                                    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
                                        <div className="shrink-0 space-y-3">
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

                                        <Separator className="my-4 shrink-0" />

                                        <ScrollArea className="min-h-0 flex-1">
                                            <div className="space-y-4 pr-4">
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
                                                        临时可信路径
                                                    </div>
                                                    {selectedTaskDetail.definition
                                                        .temporary_trusted_paths.length > 0 ? (
                                                        <div className="flex flex-wrap gap-2">
                                                            {selectedTaskDetail.definition.temporary_trusted_paths.map(
                                                                (path) => (
                                                                    <Badge
                                                                        key={path}
                                                                        variant="outline"
                                                                        className="max-w-full break-all whitespace-normal"
                                                                    >
                                                                        {path}
                                                                    </Badge>
                                                                )
                                                            )}
                                                        </div>
                                                    ) : (
                                                        <div className="text-sm text-muted-foreground">
                                                            未追加
                                                        </div>
                                                    )}
                                                </div>
                                                <div>
                                                    <div className="mb-1 text-xs font-medium text-muted-foreground">
                                                        临时 Skills
                                                    </div>
                                                    {selectedTaskDetail.definition
                                                        .temporary_skill_identifiers.length > 0 ? (
                                                        <div className="flex flex-wrap gap-2">
                                                            {selectedTaskDetail.definition.temporary_skill_identifiers.map(
                                                                (identifier) => (
                                                                    <Badge
                                                                        key={identifier}
                                                                        variant="outline"
                                                                        className="max-w-full break-all whitespace-normal"
                                                                    >
                                                                        {availableSkillNameByIdentifier.get(
                                                                            identifier
                                                                        ) ?? identifier}
                                                                    </Badge>
                                                                )
                                                            )}
                                                        </div>
                                                    ) : (
                                                        <div className="text-sm text-muted-foreground">
                                                            未追加
                                                        </div>
                                                    )}
                                                </div>
                                                <div>
                                                    <div className="mb-1 text-xs font-medium text-muted-foreground">
                                                        结果摘要
                                                    </div>
                                                    <div className="whitespace-pre-wrap rounded-md border bg-muted/20 p-3 text-sm">
                                                        {selectedTaskSummary || "暂无摘要"}
                                                    </div>
                                                </div>
                                                {!selectedTaskSummary ? (
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
                                                ) : null}
                                            </div>
                                        </ScrollArea>
                                    </div>
                                ) : (
                                    <div className="flex h-48 items-center justify-center text-sm text-muted-foreground">
                                        暂无任务详情。
                                    </div>
                                )}
                            </div>
                        </div>
                    </div>
                ) : null}
                <Dialog open={isTaskDialogOpen} onOpenChange={handleTaskDialogOpenChange}>
                    <DialogContent className="max-w-2xl">
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
                            <div className="space-y-2">
                                <div className="text-sm font-medium">临时可信路径</div>
                                <p className="text-xs text-muted-foreground">
                                    仅对本次子任务额外生效，会与该助手已有信任路径并集叠加。
                                </p>
                                <div className="flex items-center gap-2">
                                    <FolderPicker
                                        value={taskTemporaryPathInput}
                                        onChange={setTaskTemporaryPathInput}
                                        placeholder="选择或输入要追加的路径"
                                        className="flex-1"
                                    />
                                    <Button
                                        type="button"
                                        variant="outline"
                                        size="sm"
                                        onClick={handleAddTemporaryTrustedPath}
                                        disabled={!taskTemporaryPathInput.trim()}
                                    >
                                        <Plus className="mr-1 h-4 w-4" />
                                        添加
                                    </Button>
                                </div>
                                {taskTemporaryTrustedPaths.length > 0 ? (
                                    <div className="flex flex-wrap gap-2">
                                        {taskTemporaryTrustedPaths.map((path) => (
                                            <Badge
                                                key={path}
                                                variant="outline"
                                                className="max-w-full break-all whitespace-normal"
                                            >
                                                {path}
                                                <button
                                                    type="button"
                                                    className="ml-2 inline-flex"
                                                    onClick={() =>
                                                        handleRemoveTemporaryTrustedPath(path)
                                                    }
                                                    aria-label={`移除路径 ${path}`}
                                                >
                                                    <X className="h-3 w-3" />
                                                </button>
                                            </Badge>
                                        ))}
                                    </div>
                                ) : (
                                    <p className="text-xs text-muted-foreground">
                                        当前未追加临时可信路径
                                    </p>
                                )}
                            </div>
                            <div className="space-y-2">
                                <div className="text-sm font-medium">临时 Skills</div>
                                <p className="text-xs text-muted-foreground">
                                    仅对本次子任务额外生效，会与该助手已有 Skills 并集叠加。
                                </p>
                                <Input
                                    value={taskSkillQuery}
                                    onChange={(event) => setTaskSkillQuery(event.target.value)}
                                    placeholder="搜索 Skill 名称或 identifier"
                                />
                                <div className="rounded-md border">
                                    <ScrollArea className="h-56">
                                        <div className="space-y-2 p-3">
                                            {loadingAvailableSkills ? (
                                                <div className="flex items-center gap-2 text-sm text-muted-foreground">
                                                    <Loader2 className="h-4 w-4 animate-spin" />
                                                    正在加载 Skills...
                                                </div>
                                            ) : filteredAvailableSkills.length > 0 ? (
                                                filteredAvailableSkills.map((skill) => {
                                                    const checked =
                                                        taskTemporarySkillIdentifiers.includes(
                                                            skill.identifier
                                                        );
                                                    return (
                                                        <label
                                                            key={skill.identifier}
                                                            className="flex cursor-pointer items-start gap-3 rounded-md border p-3"
                                                        >
                                                            <Checkbox
                                                                checked={checked}
                                                                onCheckedChange={(value) =>
                                                                    handleToggleTemporarySkill(
                                                                        skill.identifier,
                                                                        value === true
                                                                    )
                                                                }
                                                            />
                                                            <div className="min-w-0 flex-1">
                                                                <div className="text-sm font-medium">
                                                                    {skill.display_name}
                                                                </div>
                                                                <div className="break-all text-xs text-muted-foreground">
                                                                    {skill.identifier}
                                                                </div>
                                                                {skill.metadata.description ? (
                                                                    <div className="mt-1 text-xs text-muted-foreground">
                                                                        {skill.metadata.description}
                                                                    </div>
                                                                ) : null}
                                                            </div>
                                                        </label>
                                                    );
                                                })
                                            ) : (
                                                <div className="text-sm text-muted-foreground">
                                                    暂无匹配的 Skills
                                                </div>
                                            )}
                                        </div>
                                    </ScrollArea>
                                </div>
                            </div>
                        </div>
                        <DialogFooter>
                            <Button
                                type="button"
                                variant="outline"
                                onClick={() => handleTaskDialogOpenChange(false)}
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
