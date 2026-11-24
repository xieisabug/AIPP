import { useAssistantItemActions } from "@/hooks/assistant/useAssistantItemActions";
import { resetButlerContactConversation, type ButlerContactResetResult } from "@/services/butlerContactService";
import ButlerGroupChat from "@/components/butler/ButlerGroupChat";
import ButlerProactiveSettings from "@/components/butler/ButlerProactiveSettings";
import ButlerProactiveCenter from "@/components/butler/ButlerProactiveCenter";
import { useButlerProactiveConfig } from "@/components/butler/useButlerProactiveConfig";
import ButlerGroupMemberPicker from "@/components/butler/ButlerGroupMemberPicker";
import type { ButlerGroupSummary } from "@/data/ButlerGroup";
import * as butlerGroups from "@/services/butlerGroupService";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { useFocusedWindow } from "@/hooks/useFocusedWindow";
import ButlerContactList, { ButlerAppActions, type ContactPreview } from "@/components/butler/ButlerContactList";
import { useContactRailWidth } from "@/components/butler/useContactRailWidth";
import { cn } from "@/utils/utils";
import { FloatingWindowControls, WINDOW_HEADER_HEIGHT, WindowControls, dragRegionProps, useFramelessTitleBar, useWindowTitleBarLayout } from "@/components/window/WindowTitleBar";
import { useContactAvatarActivity } from "@/components/avatar/useContactAvatarActivity";
import ButlerAvatar from "@/components/butler/ButlerAvatar";
import ConversationSearchDialog from "@/components/ConversationSearchDialog";
import { searchButlerMessages } from "@/services/butlerSearchService";
import type { ButlerMessageSearchHit } from "@/data/Conversation";
import ButlerProfileDialog, { type ProfileModelChoice, type ProfileModelField } from "@/components/butler/ButlerProfileDialog";
import { appearanceKey, ButlerAppearanceContext, parseAppearance, type ButlerAppearance } from "@/components/butler/butlerAppearance";
import "@/components/butler/butlerDesktop.css";
import { toast } from "sonner";
import { openScheduleTask } from "@/lib/scheduleTaskFocus";
import type { ScheduledTask } from "@/components/schedule/scheduleTypes";
import {
    Check,
    ChevronLeft,
    ExternalLink,
    History,
    Loader2,
    MessageSquareText,
    Pencil,
    RefreshCw,
    ScrollText,
    X,
} from "lucide-react";

import ConversationUI, {
    ConversationUIRef,
    type InlineInteractionItem,
    type PreviewFileContextSelection,
} from "@/components/ConversationUI";
import IconButton from "@/components/IconButton";
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
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
} from "@/components/ui/dialog";
import {
    AlertDialog,
    AlertDialogAction,
    AlertDialogCancel,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { ExperimentalConfigForm } from "@/components/config/feature/forms/ExperimentalConfigForm";
import {
    buildExperimentalConfigFormValues,
    EXPERIMENTAL_CONFIG_DEFAULT_VALUES,
    ExperimentalConfigFormState,
    saveExperimentalConfigValues,
} from "@/components/config/feature/forms/experimentalConfigShared";
import { ConversationStatsDialog } from "@/components/token-statistics";
import DeepSeekBalanceBadge from "@/components/conversation/DeepSeekBalanceBadge";
import { ButlerOnboardingWizard } from "@/components/butler/ButlerOnboardingWizard";
import { ButlerQuoteContext, type ButlerQuoteTarget } from "@/components/butler/ButlerQuote";
import { taskQuestionItems, useButlerTaskQuestions } from "@/components/butler/butlerTaskQuestions";
import { buildButlerWorkspaceConfig } from "@/components/butler/butlerWorkspaceConfig";
import { AssistantListItem, type AssistantDetail } from "@/data/Assistant";
import {
    ButlerMainLoadResponse,
    ButlerTaskListItem,
    ButlerTaskResultAvailableEvent,
} from "@/data/Butler";
import { ButlerLiveCardsContext } from "@/components/butler/ButlerPresentationContext";
import { useAskUserQuestion, usePreviewFile } from "@/hooks/useInlineInteraction";
import { useVaultUseRequests } from "../hooks/useVaultUseRequests";
import { useVaultInputRequests } from "../hooks/useVaultInputRequests";
import { vaultInlineItems } from "../components/conversation/VaultUseCard";
import { vaultInputInlineItems } from "../components/conversation/VaultInputCard";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import { useAppShortcuts } from "@/hooks/useAppShortcuts";
import { useTheme } from "@/hooks/useTheme";
import { useAcpPermission, useOperationPermission } from "@/hooks/useOperationPermission";
import { useApprovalSurface } from "@/hooks/useApprovalAttention";
import { useAcpElicitation } from "@/hooks/useAcpElicitation";
import { AntiLeakageProvider } from "@/contexts/AntiLeakageContext";
import { pluginRuntime } from "@/services/PluginRuntime";

type ButlerScheduledTask = Pick<ScheduledTask, "id" | "needsAttention" | "butlerMode">;

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

/** 助手类型名称（与新增助手表单的内置类型一致；插件类型显示编号）。 */
const ASSISTANT_TYPE_NAMES: Record<number, string> = { 0: "普通对话助手", 4: "智能体助手" };

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
    const resettingMainConversationRef = useRef(false);
    const mainConversationIdRef = useRef("");
    const [pluginList, setPluginList] = useState<any[]>([]);
    const [assistants, setAssistants] = useState<AssistantListItem[]>([]);
    const [mainConversationId, setMainConversationId] = useState<string>("");
    const [tasks, setTasks] = useState<ButlerTaskListItem[]>([]);
    const [contact, setContact] = useState<{ assistantId: number; conversationId: number } | null>(null);
    const contactRequest = useRef(0);
    // 群聊和联系人在同一个列表里：选中群聊时主区域显示群聊，联系人与历史视图让位。
    const [groups, setGroups] = useState<ButlerGroupSummary[]>([]);
    const [groupsLoaded, setGroupsLoaded] = useState(false);
    const [groupId, setGroupId] = useState<number | null>(null);
    const [createGroupOpen, setCreateGroupOpen] = useState(false);
    const [deleteGroupTarget, setDeleteGroupTarget] = useState<ButlerGroupSummary | null>(null);
    const groupsRequest = useRef(0);
    const loadGroups = useCallback(async () => {
        const request = ++groupsRequest.current;
        try {
            const rows = await butlerGroups.listButlerGroups();
            if (request !== groupsRequest.current) return;
            setGroups(rows);
            setGroupId(current => current !== null && !rows.some(group => group.id === current) ? null : current);
        } catch (error) {
            console.warn("读取群聊列表失败", error);
        } finally {
            if (request === groupsRequest.current) setGroupsLoaded(true);
        }
    }, []);
    const [detailsConversation, setDetailsConversation] = useState<string | null>(null);
    // 搜索打开的归档主会话/派工任务：只读查看，返回后恢复原来的主会话或联系人
    const [historyView, setHistoryView] = useState<{ conversationId: string; title: string } | null>(null);
    const activeConversationId = historyView?.conversationId
        ?? (contact ? String(contact.conversationId) : mainConversationId);
    const activeAssistant = assistants.find(assistant => assistant.id === contact?.assistantId);
    // 联系人带上描述、类型与模型，和新增助手的字段一致；总管家只改名字与形象（模型在管家设置里）。
    const [profileTarget, setProfileTarget] = useState<{
        id?: number; name: string; conversationId?: number; description?: string; model?: ProfileModelField;
    } | null>(null);
    // 顶部头像与联系人右键「编辑」共用：不传 id 时编辑当前联系人，未选联系人时编辑总管家。
    const openProfile = async (assistantId = contact?.assistantId) => {
        if (assistantId === undefined) {
            setProfileTarget({ name: butlerDisplayName, conversationId: Number(mainConversationId) || undefined });
            return;
        }
        // 记忆页按该助手的当前私聊读取；还没有私聊时记忆页说明原因。
        const conversationId = contact?.assistantId === assistantId
            ? contact.conversationId
            : previews.find(preview => preview.assistantId === assistantId)?.conversationId;
        try {
            const detail = await invoke<AssistantDetail>("get_assistant", { assistantId });
            const model = detail.model[0];
            setProfileTarget({
                id: assistantId, name: detail.assistant.name, conversationId,
                description: detail.assistant.description ?? "",
                model: {
                    assistantType: detail.assistant.assistant_type,
                    typeName: ASSISTANT_TYPE_NAMES[detail.assistant.assistant_type] ?? `类型 ${detail.assistant.assistant_type}`,
                    current: model && model.provider_id > 0 ? { modelCode: model.model_code, providerId: model.provider_id } : null,
                },
            });
        } catch (error) {
            toast.error(`读取助手 ${assistantId} 的资料失败：${error instanceof Error ? error.message : String(error)}`);
        }
    };
    const saveProfile = async (name: string, appearance: ButlerAppearance, description?: string, modelChoice?: ProfileModelChoice) => {
        if (!profileTarget) return;
        const target = profileTarget;
        let nameSaved = false;
        try {
            const latest = await loadFeatureConfig();
            if (target.id === undefined) {
                await saveFeatureConfig("experimental", {
                    ...Object.fromEntries(latest.get("experimental") ?? []),
                    butler_display_name: name,
                });
            } else {
                const detail = await invoke<AssistantDetail>("get_assistant", { assistantId: target.id });
                const [firstModel, ...otherModels] = detail.model;
                const changedModel = modelChoice && modelChoice.providerId > 0
                    && (modelChoice.modelCode !== firstModel?.model_code || modelChoice.providerId !== firstModel?.provider_id)
                    ? modelChoice : null;
                const nextDescription = description ?? detail.assistant.description ?? "";
                if (name !== detail.assistant.name || nextDescription !== (detail.assistant.description ?? "") || changedModel) {
                    // 模型的写法与助手配置页一致：替换第一个模型的代码与提供商，保留其 id 与别名，其余模型不动。
                    const nextModel = changedModel ? [{
                        id: firstModel?.id ?? 0,
                        assistant_id: firstModel?.assistant_id ?? target.id,
                        model_code: changedModel.modelCode,
                        provider_id: changedModel.providerId,
                        alias: firstModel?.alias ?? "",
                    }, ...otherModels] : detail.model;
                    await invoke("save_assistant", { assistantDetail: {
                        ...detail, assistant: { ...detail.assistant, name, description: nextDescription }, model: nextModel,
                    } });
                    setAssistants(previous => previous.map(assistant => assistant.id === target.id ? { ...assistant, name } : assistant));
                }
            }
            nameSaved = true;
            await saveFeatureConfig("butler_appearance", {
                [appearanceKey(target.id)]: JSON.stringify(appearance),
            });
            toast.success(target.id === undefined ? "名字与形象已保存" : "助手资料与形象已保存");
        } catch (error) {
            throw new Error(`${nameSaved ? "资料已保存，但形象保存失败" : "保存助手资料失败"}（${target.id === undefined ? "总管家" : `助手 ${target.id}`}）：${error instanceof Error ? error.message : String(error)}`);
        }
    };
    const [rosterError, setRosterError] = useState("");
    const rosterRequest = useRef(0);
    const [previews, setPreviews] = useState<ContactPreview[]>([]);
    const restoreContact = useRef(true);
    const previewRequest = useRef(0);
    const loadPreviews = useCallback(async () => {
        const request = ++previewRequest.current;
        try {
            const rows = await invoke<ContactPreview[]>("list_butler_contact_previews");
            await Promise.all(rows.map(row => butlerGroups.importLegacyRead(row.conversationId, false)));
            if (request === previewRequest.current) setPreviews(rows);
        } catch (error) {
            console.warn("读取联系人摘要失败", error);
        }
    }, []);
    useEffect(() => {
        if (!mainConversationId) return;
        void loadPreviews();
        window.addEventListener("focus", loadPreviews);
        return () => window.removeEventListener("focus", loadPreviews);
    }, [mainConversationId, loadPreviews]);
    useEffect(() => {
        let timer: ReturnType<typeof setTimeout>;
        let disposed = false;
        const schedule = () => {
            if (disposed) return;
            clearTimeout(timer);
            timer = setTimeout(loadPreviews, 200);
        };
        const activity = listen<{ kind: string }>("conversation_list_activity", ({ payload }) => {
            if (payload.kind === "stream_complete") schedule();
        });
        const reply = listen("butler_reply_received", schedule);
        const scheduled = listen("scheduled_task_delivered", schedule);
        const proactive = listen("butler_proactive_changed", schedule);
        const peer = listen("butler_peer_changed", schedule);
        const task = listen("butler_task_updated", schedule);
        const sync = listen("conversation_data_changed", schedule);
        const read = listen("butler_read_changed", schedule);
        return () => {
            disposed = true;
            clearTimeout(timer);
            for (const listener of [activity, reply, scheduled, proactive, peer, task, sync, read]) void listener.then(stop => stop()).catch(console.warn);
        };
    }, [loadPreviews]);
    const focused = useFocusedWindow();
    const readingConversation = focused && !historyView && groupId === null ? Number(activeConversationId) : null;
    const activePreview = previews.find(row => row.conversationId === readingConversation);
    useEffect(() => {
        if (activePreview?.unread && activePreview.replyId && document.hasFocus() && !document.hidden) {
            void butlerGroups.markRepliesRead(activePreview.conversationId, activePreview.replyId, false).catch(console.warn);
        }
    }, [activePreview]);
    const selectContact = useCallback(async (assistant?: AssistantListItem) => {
        restoreContact.current = false;
        setHistoryView(null);
        setGroupId(null);
        localStorage.removeItem("butler-last-group");
        const request = ++contactRequest.current;
        if (!assistant) {
            localStorage.removeItem("butler-last-contact");
            setContact(null);
            return;
        }
        try {
            const conversationId = await invoke<number>("open_butler_contact", { assistantId: assistant.id });
            if (request === contactRequest.current) {
                localStorage.setItem("butler-last-contact", String(assistant.id));
                setContact({ assistantId: assistant.id, conversationId });
                void loadPreviews();
            }
        } catch (error) {
            if (request === contactRequest.current) toast.error(`无法打开${assistant.name}的私聊：${error}`);
        }
    }, [loadPreviews]);
    const selectGroup = useCallback((group: ButlerGroupSummary) => {
        restoreContact.current = false;
        ++contactRequest.current;
        setHistoryView(null);
        setContact(null);
        setGroupId(group.id);
        localStorage.setItem("butler-last-group", String(group.id));
    }, []);
    useEffect(() => {
        if (!mainConversationId) return;
        let timer: ReturnType<typeof setTimeout> | undefined;
        void loadGroups();
        const refresh = () => {
            clearTimeout(timer);
            timer = setTimeout(() => void loadGroups(), 150);
        };
        const subscription = butlerGroups.onButlerGroupChanged(refresh);
        const sync = listen("conversation_data_changed", refresh);
        const read = listen("butler_read_changed", refresh);
        window.addEventListener("focus", loadGroups);
        return () => {
            clearTimeout(timer);
            window.removeEventListener("focus", loadGroups);
            for (const listener of [subscription, sync, read]) void listener.then(stop => stop()).catch(console.warn);
        };
    }, [mainConversationId, loadGroups]);
    const [searchOpen, setSearchOpen] = useState(false);
    const [pendingScroll, setPendingScroll] = useState<{ conversationId: string; messageId: number } | null>(null);
    useEffect(() => {
        const open = (event: Event) => {
            const { conversation, position } = (event as CustomEvent<{ conversation: number; position?: number }>).detail;
            if (!Number.isSafeInteger(conversation) || conversation <= 0) return;
            setIsSettingsDialogOpen(false);
            setProactiveCenterOpen(false);
            const group = groups.find(g => g.id === conversation);
            if (group) {
                selectGroup(group);
                if (position) setPendingScroll({conversationId:String(conversation),messageId:position});
                return;
            }
            const conversationId = String(conversation);
            setGroupId(null);
            if (position) setPendingScroll({ conversationId, messageId: position });
            if (conversationId === mainConversationId) { void selectContact(undefined); return; }
            const target = assistants.find(a => a.id === previews.find(p => p.conversationId === conversation)?.assistantId);
            if (target) { void selectContact(target); return; }
            setHistoryView({ conversationId, title: "查看会话" });
        };
        window.addEventListener("butler-proactive-open", open);
        return () => window.removeEventListener("butler-proactive-open", open);
    }, [groups, selectGroup, mainConversationId, selectContact, assistants, previews]);
    const searchMessages = useCallback(
        (query: string, limit: number, offset: number) =>
            searchButlerMessages(query, limit, offset, mainConversationId ? Number(mainConversationId) : undefined),
        [mainConversationId],
    );
    const handleSearchSelect = useCallback((hit: ButlerMessageSearchHit) => {
        const conversationId = String(hit.conversation_id);
        const assistant = hit.conversation_kind === "butler_contact"
            ? assistants.find(item => item.id === hit.assistant_id)
            : undefined;
        if (assistant || conversationId === mainConversationId) {
            // 当前主会话与联系人私聊在管家窗口内打开，切换完成后再定位消息
            if (hit.message_id) setPendingScroll({ conversationId, messageId: hit.message_id });
            void selectContact(assistant);
            return;
        }
        // 已归档主会话、派工任务（以及已删除助手的私聊）在管家窗口内只读打开
        setGroupId(null);
        setHistoryView({ conversationId, title: hit.conversation_name });
        if (hit.message_id) setPendingScroll({ conversationId, messageId: hit.message_id });
    }, [assistants, mainConversationId, selectContact]);
    useEffect(() => {
        if (!pendingScroll || pendingScroll.conversationId !== activeConversationId) return;
        let attempts = 0;
        const handle = window.setInterval(() => {
            attempts += 1;
            if (conversationUIRef.current || attempts >= 10) {
                conversationUIRef.current?.scrollToMessage(pendingScroll.messageId);
                setPendingScroll(null);
                window.clearInterval(handle);
            }
        }, 200);
        return () => window.clearInterval(handle);
    }, [pendingScroll, activeConversationId]);
    useEffect(() => {
        if (!restoreContact.current || !mainConversationId || !assistants.length || !groupsLoaded) return;
        restoreContact.current = false;
        const lastGroup = groups.find(group => group.id === Number(localStorage.getItem("butler-last-group")));
        if (lastGroup) {
            selectGroup(lastGroup);
            return;
        }
        const lastId = Number(localStorage.getItem("butler-last-contact"));
        const last = assistants.find(assistant => assistant.id === lastId);
        if (last) void selectContact(last);
    }, [assistants, mainConversationId, selectContact, groups, groupsLoaded, selectGroup]);
    const [loadingMain, setLoadingMain] = useState(true);
    const [resettingMainConversation, setResettingMainConversation] = useState(false);
    const [loadingFeishuStatus, setLoadingFeishuStatus] = useState(false);
    const [isStatsDialogOpen, setIsStatsDialogOpen] = useState(false);
    const [isSettingsDialogOpen, setIsSettingsDialogOpen] = useState(false);
    const [proactiveCenterOpen, setProactiveCenterOpen] = useState(false);
    const proactive = useButlerProactiveConfig();
    const [loadError, setLoadError] = useState<string | null>(null);
    const [feishuStatus, setFeishuStatus] = useState<FeishuRuntimeStatus | null>(null);
    const [isMainConversationFeishuBound, setIsMainConversationFeishuBound] =
        useState(false);
    const [scheduledTasks, setScheduledTasks] = useState<ButlerScheduledTask[]>([]);
    const [isOnboardingOpen, setIsOnboardingOpen] = useState(false);
    const onboardingAutoTriggeredRef = useRef(false);
    const butlerSettingsForm = useForm<ExperimentalConfigFormState>({
        defaultValues: { ...EXPERIMENTAL_CONFIG_DEFAULT_VALUES },
    });

    const conversationIdNumber = mainConversationId ? parseInt(mainConversationId, 10) : undefined;
    const chatConversationIds = useMemo(() => {
        const ids = new Set<number>();
        if (conversationIdNumber !== undefined) {
            ids.add(conversationIdNumber);
        }
        if (contact) ids.add(contact.conversationId);
        previews.forEach(row => ids.add(row.conversationId));
        tasks.forEach((task) => {
            ids.add(task.task_conversation_id);
        });
        return Array.from(ids);
    }, [conversationIdNumber, contact, previews, tasks]);
    // 群成员的审批同样在窗口级弹出；成员的提问显示在群聊消息流里（见 ButlerGroupChat），这里不重复接管。
    const permissionConversationIds = useMemo(() => {
        const ids = new Set(chatConversationIds);
        groups.forEach(group => group.members.forEach(member => ids.add(member.conversation_id)));
        return Array.from(ids);
    }, [chatConversationIds, groups]);
    // 审批弹窗覆盖全部管家会话；工具调用确认卡片只在打开的私聊/主会话里（群聊不显示工具调用）。
    useApprovalSurface(groupId === null ? [Number(activeConversationId)] : [], permissionConversationIds);

    const {
        pendingRequest,
        isDialogOpen,
        decisionError,
        isSubmitting,
        handleDecision,
    } = useOperationPermission({
        conversationId: contact?.conversationId ?? conversationIdNumber,
        conversationIds: permissionConversationIds,
    });
    const {
        pendingRequest: pendingAcpRequest,
        isDialogOpen: isAcpDialogOpen,
        decisionError: acpDecisionError,
        isSubmitting: isAcpSubmitting,
        handleDecision: handleAcpDecision,
    } = useAcpPermission({
        conversationId: contact?.conversationId ?? conversationIdNumber,
        conversationIds: permissionConversationIds,
    });
    const {
        questionRequest: elicitationQuestionRequest,
        decisionError: elicitationDecisionError,
        handleSubmit: handleElicitationSubmit,
        handleCancel: handleElicitationCancel,
    } = useAcpElicitation({
        conversationId: contact?.conversationId ?? conversationIdNumber,
        conversationIds: chatConversationIds,
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
        conversationId: contact?.conversationId ?? conversationIdNumber,
    });
    const { requests: vaultUseRequests, respond: respondVaultUse } = useVaultUseRequests(
        contact?.conversationId ?? conversationIdNumber ?? null,
    );
    const vaultInput = useVaultInputRequests(contact?.conversationId ?? conversationIdNumber ?? null);
    const {
        pendingRequest: pendingPreviewFileRequest,
        isDialogOpen: isPreviewFileDialogOpen,
        callId: previewFileCallId,
        managedCallId: managedPreviewFileCallId,
        sourceRequestId: previewSourceRequestId,
        messageId: previewFileMessageId,
        handleOpenChange: handlePreviewFileOpenChange,
        reopenPersistedPreview: reopenPersistedPreviewFile,
    } = usePreviewFile({
        conversationId: groupId === null && activeConversationId ? Number(activeConversationId) : undefined,
    });

    const handlePreviewFileContextSelection = useCallback((selection: PreviewFileContextSelection) => {
        if (selection.messageId) {
            conversationUIRef.current?.scrollToMessage(selection.messageId);
        }
        reopenPersistedPreviewFile(selection.callId);
    }, [reopenPersistedPreviewFile]);

    const { questions: taskQuestions, submit: submitTaskQuestion, cancel: cancelTaskQuestion } =
        useButlerTaskQuestions(historyView ? null : contact?.conversationId ?? null);
    // 聊天式引用回复：切换会话时清除，避免把引用带到另一位助手。
    const [quote, setQuote] = useState<ButlerQuoteTarget | null>(null);
    useEffect(() => setQuote(null), [activeConversationId]);
    const quoteState = useMemo(() => ({ quote, setQuote }), [quote]);
    const inlineInteractionItems: InlineInteractionItem[] = [];
    inlineInteractionItems.push(...vaultInlineItems(vaultUseRequests, respondVaultUse));
    inlineInteractionItems.push(...vaultInputInlineItems(vaultInput.requests, vaultInput.submit, vaultInput.decline));
    if (isAskUserDialogOpen && pendingAskUserRequest) {
        inlineInteractionItems.push({
            key: `butler-ask-user-${pendingAskUserRequest.request_id}`,
            requestId: pendingAskUserRequest.request_id,
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
            requireCallMarker: previewFileCallId != null && detailsConversation === activeConversationId,
            requestId: previewSourceRequestId ?? pendingPreviewFileRequest.request_id,
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
    // 执行助手在独立任务里提出的问题，显示在它自己的联系人窗口，答案回到原问题与原执行。
    inlineInteractionItems.push(...taskQuestionItems(taskQuestions, activeAssistant?.name ?? "助手", submitTaskQuestion, cancelTaskQuestion));
    const hasInlineInteraction = inlineInteractionItems.length > 0;

    const loadAssistants = useCallback(async () => {
        const request = ++rosterRequest.current;
        try {
            const result = await invoke<AssistantListItem[]>("get_assistants");
            if (rosterRequest.current !== request) return;
            setAssistants(result);
            setRosterError("");
            setContact(active => active && !result.some(a => a.id === active.assistantId) ? null : active);
        } catch (error) {
            if (rosterRequest.current === request) setRosterError(String(error));
        }
    }, []);

    // 重开联系人私聊（与总管家「重开新会话」一致）：旧私聊归档，仍停留在该助手时切到新私聊。
    const [resettingContactId, setResettingContactId] = useState<number | null>(null);
    const showResetContact = useCallback((assistantId: number, result: Omit<ButlerContactResetResult, "stopNotes">) => {
        result.archivedConversationIds.forEach(id => localStorage.removeItem(`butler-read-${id}`));
        setContact(active => active?.assistantId === assistantId && active.conversationId < result.conversationId
            ? { assistantId, conversationId: result.conversationId }
            : active);
    }, []);
    useEffect(() => {
        const listener = listen<Omit<ButlerContactResetResult, "stopNotes"> & { assistantId: number }>(
            "butler_contact_reset", ({ payload }) => {
                showResetContact(payload.assistantId, payload);
                void loadPreviews();
            }
        );
        return () => { void listener.then(stop => stop()).catch(console.warn); };
    }, [loadPreviews, showResetContact]);
    const resetContactConversation = useCallback(async (assistantId: number) => {
        setResettingContactId(assistantId);
        try {
            const result = await resetButlerContactConversation(assistantId);
            showResetContact(assistantId, result);
            if (result.stopNotes.length) toast.info(`已停止旧私聊上进行中的工作：${result.stopNotes.join("；")}`);
        } finally {
            setResettingContactId(null);
        }
    }, [showResetContact]);
    const handleResetContactConversation = useCallback(async (assistantId: number) => {
        try {
            await resetContactConversation(assistantId);
            toast.success("旧私聊已归档，已开始新会话");
        } catch (error) {
            toast.error(`重开新会话失败（助手 ${assistantId}）：${error instanceof Error ? error.message : String(error)}`);
        }
    }, [resetContactConversation]);

    const assistantItemActions = useAssistantItemActions({
        // 右键「编辑」与顶部头像打开同一个三页资料框（资料、形象、记忆），不用配置页的精简编辑框。
        onEdit: assistant => openProfile(assistant.id),
        onReset: assistant => resetContactConversation(assistant.id),
    });

    useEffect(() => {
        let active = true;
        const refresh = () => { if (active) void loadAssistants(); };
        const listener = listen("assistant_list_changed", refresh);
        void listener.then(refresh).catch(error => { if (active) setRosterError(String(error)); });
        window.addEventListener("focus", refresh);
        return () => {
            active = false;
            ++rosterRequest.current;
            window.removeEventListener("focus", refresh);
            void listener.then(stop => stop()).catch(console.warn);
        };
    }, [loadAssistants]);

    const applyMainConversationResult = useCallback((result: ButlerMainLoadResponse) => {
        setLoadError(null);
        setMainConversationId(String(result.conversation.id));
        setTasks(sortTasks(result.tasks));
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
                error instanceof Error ? error.message : String(error);
            console.error("[ButlerExperimentWindow] Failed to load main conversation:", error);
            if (!silentError) {
                setLoadError(message);
            }
        } finally {
            if (requestId === mainLoadRequestRef.current) {
                setLoadingMain(false);
            }
        }
    }, [applyMainConversationResult]);

    useEffect(() => {
        const subscription = listen("butler_main_slot_changed", () => void loadMainConversation());
        return () => { void subscription.then(stop => stop()).catch(console.warn); };
    }, [loadMainConversation]);

    useEffect(() => {
        resettingMainConversationRef.current = resettingMainConversation;
    }, [resettingMainConversation]);

    useEffect(() => {
        mainConversationIdRef.current = mainConversationId;
    }, [mainConversationId]);

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
            // 管家模式下各助手创建的任务（结果发到助手私聊）都在这里列出
            const result = await invoke<ButlerScheduledTask[]>("list_scheduled_tasks");
            setScheduledTasks(result.filter((task) => task.butlerMode));
        } catch (error) {
            console.error("[ButlerExperimentWindow] Failed to load scheduled tasks:", error);
        }
    }, [mainConversationId]);

    // 定时任务入口与聊天窗口一致：侧栏底部按钮打开定时任务窗口；有管家任务需要处理时直接定位到它
    const scheduleAttentionTask = scheduledTasks.find((task) => task.needsAttention);
    const openButlerSchedule = useCallback(() => {
        const open = scheduleAttentionTask ? openScheduleTask(scheduleAttentionTask.id) : invoke("open_schedule_window");
        void Promise.resolve(open).catch((error) => {
            console.error("[ButlerExperimentWindow] Failed to open schedule window:", error);
            toast.error("打开定时任务失败");
        });
    }, [scheduleAttentionTask]);

    useEffect(() => {
        void loadMainConversation({ showLoading: true });
    }, [loadMainConversation]);

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
            listen("scheduled_task_changed", () => {
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
            setIsStatsDialogOpen(false);
        });

        return () => {
            unlistenHidden.then((unlisten) => unlisten()).catch(console.warn);
        };
    }, []);

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
                void loadMainConversation();
            }
        );

        return () => {
            unlistenReset.then((unlisten) => unlisten()).catch(console.warn);
        };
    }, [loadMainConversation]);

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
            }),
            listen<ButlerTaskResultAvailableEvent>(
                "butler_task_result_available",
                ({ payload }) => {
                    if (payload.task.butler_conversation_id !== conversationIdNumber) {
                        return;
                    }
                    setTasks((current) => upsertTask(current, payload.task));
                }
            ),

        ];

        return () => {
            taskListeners.forEach((listener) => {
                listener.then((unlisten) => unlisten()).catch(console.warn);
            });
        };
    }, [conversationIdNumber]);

    const handleResetMainConversation = useCallback(async () => {
        resettingMainConversationRef.current = true;
        setResettingMainConversation(true);
        try {
            const result = await invoke<ButlerMainLoadResponse>(
                "reset_butler_main_conversation"
            );
            applyMainConversationResult(result);
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
    }, [applyMainConversationResult]);

    const handleOpenSettings = useCallback(() => {
        void proactive.reload();
        loadFeatureConfig()
            .then((latestConfig) => {
                butlerSettingsForm.reset(buildExperimentalConfigFormValues(latestConfig));
                setIsSettingsDialogOpen(true);
            })
            .catch((error) => {
                console.error("[ButlerExperimentWindow] Failed to load settings:", error);
                toast.error("加载总管家设置失败");
            });
    }, [butlerSettingsForm, loadFeatureConfig, proactive.reload]);

    // 总管家设置只覆盖 Butler 相关项；模型、助手等通用配置在 AIPP 主设置窗口里
    const handleOpenAppSettings = useCallback(() => {
        invoke("open_config_window").catch((error) => {
            console.error("[ButlerExperimentWindow] Failed to open AIPP settings window:", error);
            toast.error(`打开 AIPP 设置窗口失败：${error instanceof Error ? error.message : String(error)}`);
        });
    }, []);

    const handleSaveButlerSettings = useCallback(async () => {
        // 主动整理先保存：它有后端校验（时间段、Open ID），不通过时整次保存直接报错
        await proactive.save();
        await saveExperimentalConfigValues(saveFeatureConfig, butlerSettingsForm.getValues());
    }, [butlerSettingsForm, saveFeatureConfig, proactive.save]);

    const toggleExecutionDetails = () => {
        conversationUIRef.current?.captureTranscriptPosition();
        setDetailsConversation(detailsConversation === activeConversationId ? null : activeConversationId);
    };

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

    // 顶部头像和联系人列表共用这一份状态，同一助手播同一套动作。
    const contactActivity = useContactAvatarActivity(mainConversationId, previews, readingConversation ? contact?.assistantId ?? 0 : -1);
    const headerAssistantId = contact?.assistantId ?? 0;
    const headerState = contactActivity.visuals.get(headerAssistantId)?.state ?? "idle";
    const contactRail = useContactRailWidth();
    const titleBarLayout = useWindowTitleBarLayout();
    const framelessTitleBar = useFramelessTitleBar();
    const drag = dragRegionProps(framelessTitleBar);

    useAppShortcuts("butler", {
        new: () => {
            if (historyView || groupId !== null || !mainConversationId || resettingMainConversation || resettingContactId !== null) {
                return;
            }
            if (contact) {
                void handleResetContactConversation(contact.assistantId);
                return;
            }
            void handleResetMainConversation();
        },
        search: () => {
            if (mainConversationId) setSearchOpen(true);
        },
        stats: handleOpenStats,
        settings: handleOpenSettings,
        toggle_sidebar: handleToggleSidebar,
        open_sidebar_window: handleOpenSidebarWindow,
    });

    // 主会话 / 历史只读视图的头部：交给 ConversationUI 渲染在会话列里，右侧详情栏才能像聊天窗口一样从窗口顶部占满；
    // showWindowControls：详情栏未展开时 Windows 窗口控制按钮排在头部末尾（展开时在详情栏头部）
    const renderButlerHeader = ({ showWindowControls }: { showWindowControls: boolean }) => (historyView ? (
                            <div
                                className="grid flex-none select-none grid-cols-[1fr_auto_1fr] items-center gap-2 border-b border-border px-3"
                                style={{ height: WINDOW_HEADER_HEIGHT }}
                                {...drag}
                                data-aipp-slot="butler-history-header"
                            >
                                <div className="flex min-w-0 items-center self-stretch" {...drag}>
                                    <Button type="button" variant="ghost" size="sm" className="gap-1 px-2"
                                        onClick={() => setHistoryView(null)} data-aipp-slot="butler-history-back">
                                        <ChevronLeft className="h-4 w-4" />
                                        返回{contact ? activeAssistant?.name ?? "私聊" : "当前会话"}
                                    </Button>
                                </div>
                                <div className="flex min-w-0 max-w-[26rem] items-center gap-2 self-stretch" {...drag}>
                                    <History className="pointer-events-none h-4 w-4 shrink-0 text-muted-foreground" />
                                    <span className="truncate text-sm font-medium" {...drag}>{historyView.title}</span>
                                    <Badge variant="outline" className="shrink-0 text-[10px] font-normal" {...drag}>只读</Badge>
                                </div>
                                <div className="flex min-w-0 items-center justify-end gap-1 self-stretch" {...drag}>
                                    <IconButton
                                        icon={detailsConversation === activeConversationId
                                            ? <MessageSquareText className="h-4 w-4 text-icon" />
                                            : <ScrollText className="h-4 w-4 text-icon" />}
                                        onClick={toggleExecutionDetails}
                                        ghost
                                        title={detailsConversation === activeConversationId ? "返回简洁聊天" : "执行记录"}
                                    />
                                    {showWindowControls && <WindowControls className="ml-2" />}
                                </div>
                            </div>
                            ) : (
                            <div
                                className="relative grid flex-none select-none grid-cols-[1fr_auto_1fr] items-center gap-2 border-b border-border px-3"
                                style={{ height: WINDOW_HEADER_HEIGHT }}
                                {...drag}
                                data-aipp-slot="butler-main-header"
                            >
                                <div className="flex min-w-0 items-center self-stretch" {...drag}>
                                    {showFeishuStatus && <Badge variant={getFeishuStatusVariant(feishuStatus)} {...drag}>
                                        {getFeishuStatusIcon(feishuStatus, loadingFeishuStatus)} 飞书
                                    </Badge>}
                                </div>
                                <button type="button" className="group flex min-w-0 max-w-[22rem] items-center gap-2 rounded-lg px-2 py-1 text-left transition-colors hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring"
                                    aria-label="编辑助手"
                                    data-aipp-slot="butler-main-identity"
                                    onClick={() => void openProfile()}
                                    title="编辑资料、形象与记忆">
                                    <ButlerAvatar id={headerAssistantId} chief={!contact} size={28} figure animated state={headerState} />
                                    <span className="truncate text-sm font-medium">{activeAssistant?.name ?? butlerDisplayName}</span>
                                    <Pencil aria-hidden="true" className="size-3 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100" />
                                </button>
                                <div className="flex min-w-0 items-center justify-end gap-1 self-stretch" {...drag}>
                                    <DeepSeekBalanceBadge
                                        conversationId={Number(activeConversationId) || null}
                                        assistantId={contact?.assistantId ?? null}
                                        className="mr-1"
                                    />
                                    <IconButton
                                        icon={detailsConversation === activeConversationId
                                            ? <MessageSquareText className="h-4 w-4 text-icon" />
                                            : <ScrollText className="h-4 w-4 text-icon" />}
                                        onClick={toggleExecutionDetails}
                                        ghost
                                        title={detailsConversation === activeConversationId ? "返回简洁聊天" : "执行记录"}
                                        dataAippSlot="butler-main-toggle-details"
                                    />
                                    <ConversationStatsDialog
                                        showCacheActivity
                                        conversationId={activeConversationId}
                                        externalOpen={isStatsDialogOpen}
                                        onExternalOpenChange={setIsStatsDialogOpen}
                                    />
                                    <IconButton
                                        icon={
                                            resettingMainConversation || resettingContactId !== null ? (
                                                <Loader2 className="h-4 w-4 animate-spin text-icon" />
                                            ) : (
                                                <RefreshCw className="h-4 w-4 text-icon" />
                                            )
                                        }
                                        onClick={() => void (contact
                                            ? handleResetContactConversation(contact.assistantId)
                                            : handleResetMainConversation())}
                                        disabled={!mainConversationId || resettingMainConversation || resettingContactId !== null}
                                        ghost
                                        title={contact ? "重开新会话（旧私聊归档，可搜索查看）" : "重开新会话"}
                                        dataAippSlot="butler-main-reset-conversation"
                                    />
                                    {showWindowControls && <WindowControls className="ml-2" />}
                                </div>
                            </div>
    ));

    return (
        <ButlerAppearanceContext.Provider value={featureConfig.get("butler_appearance") ?? new Map()}>
        <AntiLeakageProvider enabled={antiLeakageEnabled}>
            <div
                className={cn("flex h-screen bg-background text-foreground", contactRail.isResizing && "select-none")}
                data-aipp-window="butler_experiment"
                data-aipp-slot="window-root"
            >
                <div
                    className="relative flex-none flex flex-col border-r border-border bg-muted/30 overflow-hidden"
                    style={{ width: contactRail.width }}
                    data-aipp-slot="butler-task-rail"
                >
                    <ButlerContactList chief={butlerDisplayName} contacts={assistants} selected={contact?.assistantId}
                        focused={focused} ready={!!mainConversationId} previews={previews} plugins={pluginList} activity={contactActivity}
                        error={rosterError} onRefresh={loadAssistants} onSelect={selectContact}
                        onSearch={() => setSearchOpen(true)} onOpenSettings={handleOpenSettings}
                        onOpenProactive={proactive.saved?.enabled ? () => setProactiveCenterOpen(true) : undefined}
                        onAssistantAction={(assistant, action) => { void assistantItemActions.run(assistant, action); }}
                        groups={groups} selectedGroup={groupId ?? undefined} onSelectGroup={selectGroup}
                        onCreateGroup={() => setCreateGroupOpen(true)} onDeleteGroup={setDeleteGroupTarget}/>
                    {titleBarLayout && (
                        // 桌面统一布局：侧栏标题栏行放列表操作，设置、更新与主动整理入口收在侧栏底部
                        <div className="flex shrink-0 items-center gap-0.5 border-t border-border px-2 py-1.5" data-aipp-slot="butler-rail-footer">
                            <ButlerAppActions ready={!!mainConversationId} onOpenSettings={handleOpenSettings}
                                onOpenProactive={proactive.saved?.enabled ? () => setProactiveCenterOpen(true) : undefined}
                                onOpenSchedule={openButlerSchedule} scheduleAttention={scheduleAttentionTask !== undefined}/>
                        </div>
                    )}
                </div>
                {/* Windows 窗口控制按钮：会话与群聊头部里自带；出错或主会话未就绪时没有头部，浮在右上角 */}
                {(loadError || !mainConversationId) && <FloatingWindowControls centerY={WINDOW_HEADER_HEIGHT / 2} right={12} />}
                <div className="relative z-20 w-0 flex-none">
                    <div
                        role="separator"
                        aria-orientation="vertical"
                        aria-label="调整联系人列表宽度"
                        aria-valuemin={contactRail.minWidth}
                        aria-valuemax={contactRail.maxWidth}
                        aria-valuenow={contactRail.width}
                        tabIndex={0}
                        title="左右拖动调整联系人列表宽度，双击恢复默认"
                        onPointerDown={contactRail.handleResizeStart}
                        onDoubleClick={contactRail.resetWidth}
                        onKeyDown={contactRail.handleResizeKeyDown}
                        data-aipp-slot="butler-task-rail-resize-handle"
                        className={cn(
                            "absolute right-0 top-0 bottom-0 z-20 w-1.5 cursor-ew-resize touch-none transition-colors focus-visible:outline-none focus-visible:bg-primary/60",
                            "hover:bg-primary/50",
                            contactRail.isResizing && "bg-primary/60"
                        )}
                    />
                </div>

                <div
                    className="flex flex-1 min-w-0 flex-col overflow-hidden bg-background"
                    data-aipp-slot="butler-main-content"
                >
                    {loadError ? (
                        <div className="flex h-full items-center justify-center p-6" {...drag}>
                            <div className="max-w-md space-y-4 rounded-xl border border-dashed border-border p-6 text-sm text-muted-foreground">
                                <div className="space-y-1">
                                    <div className="text-base font-semibold text-foreground">
                                        无法打开总管家实验窗口
                                    </div>
                                    <p>{loadError}</p>
                                </div>
                                <Button type="button" variant="outline" disabled={loadingMain}
                                    onClick={() => void loadMainConversation({ showLoading: true })}>
                                    {loadingMain ? "正在重试…" : "重试"}
                                </Button>
                                <Button
                                    type="button"
                                    variant="outline"
                                    onClick={handleOpenSettings}
                                >
                                    打开设置
                                </Button>
                            </div>
                        </div>
                    ) : mainConversationId && groupId !== null ? (
                        <ButlerGroupChat key={groupId} groupId={groupId} chiefName={butlerDisplayName} assistants={assistants}
                            sourceSeq={pendingScroll?.conversationId===String(groupId) ? pendingScroll.messageId : undefined}
                            onSourceLocated={() => setPendingScroll(null)}
                            onDeleted={() => { setGroupId(null); localStorage.removeItem("butler-last-group"); void loadGroups(); }} />
                    ) : mainConversationId ? (
                        <>
                            <div className="min-h-0 flex-1">
                                <ButlerLiveCardsContext.Provider value={historyView ? {} : { file: managedPreviewFileCallId, question: askUserCallId }}>
                                <ButlerQuoteContext.Provider value={historyView ? null : quoteState}>
                                <ConversationUI
                                    key={activeConversationId}
                                    ref={conversationUIRef}
                                    conversationId={activeConversationId}
                                    onChangeConversationId={() => undefined}
                                    pluginList={pluginList}
                                    renderHeader={renderButlerHeader}
                                    showWindowControls
                                    allowRename={false}
                                    allowDelete={false}
                                    inlineInteractionItems={historyView ? undefined : inlineInteractionItems}
                                    inlineInteractionVisible={!historyView && hasInlineInteraction}
                                    allowFeishuDebugResend={
                                        !contact && !historyView && showFeishuStatus && isMainConversationFeishuBound
                                    }
                                    readOnly={!!historyView}
                                    showExecutionDetails={detailsConversation === activeConversationId}
                                    windowLabel="butler_experiment"
                                    busySendBehavior="interrupt"
                                    onPreviewFileContextClick={handlePreviewFileContextSelection}
                                />
                                </ButlerQuoteContext.Provider>
                                </ButlerLiveCardsContext.Provider>
                            </div>
                        </>
                    ) : (
                        <div className="flex h-full select-none items-center justify-center text-sm text-muted-foreground" {...drag}>
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
                    <CompactDialogContent className="sm:max-w-3xl">
                        <CompactDialogHeader
                            title="总管家实验设置"
                            description="在当前工作台内调整 Butler 相关实验配置，包括模型、上下文压缩、可信工作区和飞书接入。"
                            actions={
                                <button
                                    type="button"
                                    onClick={handleOpenAppSettings}
                                    className="inline-flex h-8 items-center gap-1 rounded-md px-2 text-xs font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                                    data-aipp-slot="butler-settings-open-app-settings"
                                >
                                    AIPP 设置
                                    <ExternalLink className="h-3 w-3" />
                                </button>
                            }
                        />
                        {/* pb-0：表单的 sticky 保存栏以滚动容器的内边距为吸附边界，留底部内边距会让它悬在弹窗底边上方并露出下面的内容 */}
                        <CompactDialogBody className="space-y-4 pb-0">
                            <p className="text-xs text-muted-foreground">
                                在当前工作台内调整 Butler 相关实验配置，包括模型、上下文压缩、可信工作区和飞书接入。其他通用配置请前往右上角「AIPP 设置」。
                            </p>
                            <ExperimentalConfigForm
                                form={butlerSettingsForm}
                                onSave={handleSaveButlerSettings}
                                scope="butler"
                                saveFeatureConfig={saveFeatureConfig}
                                onConfigRefresh={handleOnboardingComplete}
                                butlerSections={
                                    <ButlerProactiveSettings
                                        value={proactive.config}
                                        error={proactive.error}
                                        onChange={proactive.setConfig}
                                        onOpenCenter={() => setProactiveCenterOpen(true)}
                                    />
                                }
                            />
                        </CompactDialogBody>
                    </CompactDialogContent>
                </Dialog>
                <ButlerProactiveCenter
                    open={proactiveCenterOpen}
                    onOpenChange={setProactiveCenterOpen}
                    enabled={proactive.saved?.enabled === true}
                />
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
            </div>
            <ConversationSearchDialog
                open={searchOpen}
                onOpenChange={setSearchOpen}
                onSelectResult={handleSearchSelect}
                search={searchMessages}
                placeholder="搜索管家、私聊与派工任务中的消息..."
                description="搜索管家模式产生的消息"
            />
            {assistantItemActions.dialogs}
            <ButlerGroupMemberPicker open={createGroupOpen} onOpenChange={setCreateGroupOpen} mode="create"
                chiefName={butlerDisplayName} assistants={assistants}
                onConfirm={async result => {
                    const created = await butlerGroups.createButlerGroup(result.name, result.assistantIds, result.includeChief);
                    await loadGroups();
                    selectGroup(created);
                }} />
            <AlertDialog open={deleteGroupTarget !== null} onOpenChange={open => { if (!open) setDeleteGroupTarget(null); }}>
                <AlertDialogContent className="flex flex-col gap-0 overflow-hidden p-0 sm:max-w-md">
                    <AlertDialogHeader className="border-b border-border px-4 py-3 text-left sm:px-5">
                        <AlertDialogTitle className="text-base font-semibold">删除群聊「{deleteGroupTarget?.name}」？</AlertDialogTitle>
                    </AlertDialogHeader>
                    <div className="px-4 py-4 sm:px-5">
                        <AlertDialogDescription className="text-sm leading-relaxed text-muted-foreground">
                            群消息和成员在本群的执行记录都会删除，成员的私聊不受影响。正在进行的回复会先停止。
                        </AlertDialogDescription>
                    </div>
                    <AlertDialogFooter className="flex-row items-center justify-end gap-2 border-t border-border bg-background px-4 pt-2.5 pb-[calc(0.625rem+env(safe-area-inset-bottom))] sm:px-5">
                        <AlertDialogCancel className="min-h-11 px-4 sm:min-h-9">取消</AlertDialogCancel>
                        <AlertDialogAction className="min-h-11 bg-destructive px-5 text-white hover:bg-destructive/90 sm:min-h-9" onClick={() => {
                            const target = deleteGroupTarget;
                            if (!target) return;
                            butlerGroups.deleteButlerGroup(target.id).then(() => {
                                localStorage.removeItem(`butler-group-read-${target.id}`);
                                if (groupId === target.id) {
                                    setGroupId(null);
                                    localStorage.removeItem("butler-last-group");
                                }
                                void loadGroups();
                            }).catch(error => toast.error(String(error)));
                        }}>删除</AlertDialogAction>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>
            {profileTarget && <ButlerProfileDialog
                butler
                assistantId={profileTarget.id}
                name={profileTarget.name}
                appearance={parseAppearance(featureConfig.get("butler_appearance")?.get(appearanceKey(profileTarget.id)))}
                description={profileTarget.description}
                model={profileTarget.model}
                memoryConversationId={profileTarget.conversationId}
                onClose={() => setProfileTarget(null)} onSave={saveProfile} />}
        </AntiLeakageProvider>
        </ButlerAppearanceContext.Provider>
    );
}

export default ButlerExperimentWindow;
