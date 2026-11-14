/**
 * InputArea 组件 - 中文输入法兼容性说明
 *
 * ===========================================
 * WebKit2 GTK 中文输入法处理特殊问题及解决方案
 * ===========================================
 *
 * 问题背景：
 * 在 Tauri 应用中使用 WebKit2 GTK 渲染引擎时，中文输入法（IME）的行为与标准浏览器不同：
 *
 * 1. **标准浏览器行为**：
 *    - 用户在中文输入法下按回车键确认候选词时，`event.isComposing` 保持为 `true`
 *    - 只有在完全确认输入后，`event.isComposing` 才变为 `false`
 *    - 这样可以正确区分"确认候选词的回车"和"提交消息的回车"
 *
 * 2. **WebKit2 GTK 的问题**：
 *    - 用户按回车键确认候选词时，`event.isComposing` 错误地变为 `false`
 *    - 这导致确认候选词的回车被误认为是要提交消息的回车
 *    - 用户在中文输入法下无法正常输入，每次确认候选词都会意外触发消息发送
 *
 * 解决方案：
 *
 * 1. **手动状态跟踪**：
 *    - 使用 `isComposingRef` 手动跟踪 IME 组合状态
 *    - 不完全依赖 `event.isComposing` 的值
 *
 * 2. **Composition 事件监听**：
 *    - `onCompositionStart`: 当开始输入中文时设置 `isComposingRef.current = true`
 *    - `onCompositionEnd`: 当完成中文输入时延迟设置 `isComposingRef.current = false`
 *
 * 3. **延迟状态更新**：
 *    - 在 `compositionend` 事件中使用 100ms 延迟更新状态
 *    - 这确保了键盘事件处理器能正确检测到组合状态
 *    - WebKit2 的事件触发时序有时不稳定，延迟可以解决时序问题
 *
 * 4. **双重检查机制**：
 *    - 同时检查 `!isComposingRef.current` 和 `!event.isComposing`
 *    - 只有两个条件都满足时才允许回车键触发消息发送
 *    - 这提供了最大的兼容性保障
 *
 * 使用场景：
 * - 主要影响使用 Tauri + WebKit2 GTK 的桌面应用
 * - 在 Linux 系统上使用中文输入法时尤其重要
 * - 对标准浏览器环境也保持兼容
 *
 * 注意事项：
 * - 如果未来 WebKit2 GTK 修复了 `isComposing` 的问题，此代码仍然兼容
 * - 延迟时间（100ms）是经过测试的最佳值，不建议随意修改
 * - 此解决方案参考了业界最佳实践（Cherry Studio 等项目的处理方式）
 */

import React, { useRef, useEffect, useState, useCallback, useId, forwardRef, useImperativeHandle, type ReactNode } from "react";
import "../../styles/InputArea.css";
import CircleButton from "../CircleButton";
import { Switch } from "@/components/ui/switch";
import { Plus, Square, ArrowUp, Loader2 } from "lucide-react";
import { AcpAvailableCommand, FileInfo } from "../../data/Conversation";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { previewMobile } from "@/mobile/navigation";
import { getCaretCoordinates } from "../../utils/caretCoordinates";
import BangCompletionList, { type BangCompletionItem } from "./BangCompletionList";
import AssistantCompletionList, { type MentionCandidate } from "./AssistantCompletionList";
import ArtifactCompletionList from "./ArtifactCompletionList";
import SlashCompletionList from "./SlashCompletionList";
import { useFileList } from "../../hooks/useFileList";
import { MOBILE_TEXTAREA_MIN_HEIGHT, mobileTextareaMaxHeight } from "@/mobile/keyboardViewport";
import { useAssistantListListener } from "../../hooks/useAssistantListListener";
import PinyinFilter, { AssistantItem, FilteredAssistant, usePinyinReady } from "../../utils/pinyinFilter";
import { ArtifactCollectionItem, FilteredArtifact } from "../../data/ArtifactCollection";
import {
    DEFAULT_SLASH_NAMESPACES,
    FilteredSlashSkill,
    SlashNamespaceItem,
    SlashSkillCompletionItem,
} from "../../data/Slash";
import {
    buildSkillInvocation,
    getSlashCompletionContext,
    type SlashCompletionContext,
} from "../../utils/slashCompletion";

// 暴露给外部的方法接口
export interface InputAreaRef {
    focus: () => void;
}

interface InputAreaProps {
    inputText: string;
    setInputText: React.Dispatch<React.SetStateAction<string>>;
    /**
     * 附件能力：不传 handleChooseFile 表示这个聊天不支持附件（如管家群聊），
     * 不显示“添加附件”按钮，粘贴也按纯文本处理。
     */
    fileInfoList?: FileInfo[] | null;
    handleChooseFile?: () => void;
    handlePaste?: (e: React.ClipboardEvent<HTMLTextAreaElement>) => void;
    handleDeleteFile?: (fileId: number) => void;
    handleSend: () => void;
    aiIsResponsing: boolean;
    placement?: "top" | "bottom";
    isMobile?: boolean;
    sidebarWidth?: number;
    sidebarVisible?: boolean;
    sendButtonIcon?: ReactNode;
    sendButtonVisual?: ReactNode;
    acpAvailableCommands?: AcpAvailableCommand[];
    /**
     * Plan 模式开关：只有支持 Plan 的 Agent 助手才传 onPlanModeToggle，
     * 开关显示在输入框下方（仅底部输入框布局）。
     */
    planMode?: boolean;
    planModeSwitching?: boolean;
    onPlanModeToggle?: () => void;
    /** 输入框顶部的附加条（如管家引用回复的引用对象），仅桌面端展示 */
    topChip?: ReactNode;
    /**
     * @ 补全的候选。不传时列出全部助手（普通聊天里 @助手 指定回答者）；
     * 群聊传入本群成员与“所有人”。需由调用方 memo，避免每次渲染重建监听。
     */
    mentionCandidates?: MentionCandidate[];
    placeholder?: string;
    /** 输入框的无障碍名称 */
    ariaLabel?: string;
}
const IMAGE_AREA_HEIGHT = 80;
const TOP_CHIP_HEIGHT = 30;
export const COMPOSER_GROW_VAR = "--aipp-composer-grow";
// 稳定的空数组引用，避免 prop 缺省时 useCallback/useEffect 依赖每轮渲染都变化
const EMPTY_ACP_COMMANDS: AcpAvailableCommand[] = [];
const NO_FILE_DELETE = () => {};

const InputArea = React.memo(
    forwardRef<InputAreaRef, InputAreaProps>(
        (
            {
                inputText,
                setInputText,
                fileInfoList = null,
                handleChooseFile,
                handlePaste,
                handleDeleteFile = NO_FILE_DELETE,
                handleSend,
                aiIsResponsing,
                placement = "bottom",
                isMobile = false,
                sidebarWidth = 0,
                sidebarVisible = false,
                sendButtonIcon,
                sendButtonVisual,
                acpAvailableCommands = EMPTY_ACP_COMMANDS,
                planMode = false,
                planModeSwitching = false,
                onPlanModeToggle,
                topChip,
                mentionCandidates,
                placeholder,
                ariaLabel,
            },
            ref
        ) => {
            // 图片区域的高度
            const textareaRef = useRef<HTMLTextAreaElement>(null);
            const rootRef = useRef<HTMLDivElement>(null);
            const planSwitchId = useId();

            // 暴露给外部的方法
            useImperativeHandle(
                ref,
                () => ({
                    focus: () => {
                        textareaRef.current?.focus();
                    },
                }),
                []
            );

            // WebKit2 GTK 中文输入法兼容性：手动跟踪 IME 组合状态
            // 因为 WebKit2 下 event.isComposing 在确认候选词时会错误地返回 false
            const isComposingRef = useRef(false);
            const [initialHeight, setInitialHeight] = useState<number | null>(null);
            const [isFocused, setIsFocused] = useState<boolean>(false);
            const [bangListVisible, setBangListVisible] = useState<boolean>(false);
            const [bangList, setBangList] = useState<BangCompletionItem[]>([]);
            const [originalBangList, setOriginalBangList] = useState<BangCompletionItem[]>([]);
            const [cursorPosition, setCursorPosition] = useState<{
                bottom: number;
                left: number;
                top: number;
            }>({ bottom: 0, left: 0, top: 0 });
            const [selectedBangIndex, setSelectedBangIndex] = useState<number>(0);

            // Assistant selection states
            const [assistantListVisible, setAssistantListVisible] = useState<boolean>(false);
            const [assistants, setAssistants] = useState<AssistantItem[]>([]);
            const [filteredAssistants, setFilteredAssistants] = useState<FilteredAssistant<MentionCandidate>[]>([]);
            const [selectedAssistantIndex, setSelectedAssistantIndex] = useState<number>(0);
            // 调用方给了候选（如群成员）就只补全这些，否则补全全部助手
            const mentionSource: MentionCandidate[] = mentionCandidates ?? assistants;

            // Artifact selection states
            const [artifactListVisible, setArtifactListVisible] = useState<boolean>(false);
            const [artifacts, setArtifacts] = useState<ArtifactCollectionItem[]>([]);
            const [filteredArtifacts, setFilteredArtifacts] = useState<FilteredArtifact[]>([]);
            const [selectedArtifactIndex, setSelectedArtifactIndex] = useState<number>(0);

            const [slashListVisible, setSlashListVisible] = useState<boolean>(false);
            const [slashMode, setSlashMode] = useState<"namespace" | "skill">("namespace");
            const [slashContext, setSlashContext] = useState<SlashCompletionContext | null>(null);
            const [slashSkills, setSlashSkills] = useState<SlashSkillCompletionItem[]>([]);
            const [filteredSlashSkills, setFilteredSlashSkills] = useState<FilteredSlashSkill[]>([]);
            const [filteredSlashNamespaces, setFilteredSlashNamespaces] =
                useState<SlashNamespaceItem[]>(DEFAULT_SLASH_NAMESPACES);
            const [selectedSlashIndex, setSelectedSlashIndex] = useState<number>(0);

            // 补全弹窗出现时才动态加载 pinyin-pro，加载完成后触发重新过滤
            const pinyinReady = usePinyinReady(
                slashListVisible || artifactListVisible || assistantListVisible,
            );

            const isMobilePlatform = useIsMobilePlatform();

            // 移动端没有独立 artifact 窗口：读取收藏内容后走单 WebView 受限预览
            const openArtifact = useCallback(
                async (artifactId: number) => {
                    try {
                        if (isMobilePlatform) {
                            const item = await invoke<{
                                name: string;
                                artifact_type: string;
                                code: string;
                            } | null>("get_artifact_by_id", { id: artifactId });
                            if (!item) {
                                throw new Error(`收藏 ${artifactId} 已不存在`);
                            }
                            previewMobile({
                                language: item.artifact_type,
                                code: item.code,
                                title: item.name,
                            });
                            return;
                        }
                        await invoke("open_artifact_window", { artifactId });
                    } catch (error) {
                        console.error("Failed to open artifact:", error);
                        toast.error(`打开收藏 ${artifactId} 失败：${String(error)}`);
                    }
                },
                [isMobilePlatform],
            );

            const handleOpenFile = (fileId: number) => {
                invoke("open_attachment_with_default_app", { id: fileId });
            };
            const { renderFiles } = useFileList(fileInfoList, handleDeleteFile, handleOpenFile);

            const updateCompletionCursorPosition = useCallback(
                (textarea: HTMLTextAreaElement, anchorIndex: number) => {
                    const cursorCoords = getCaretCoordinates(textarea, anchorIndex);
                    const rect = textarea.getBoundingClientRect();
                    const style = window.getComputedStyle(textarea);
                    const paddingTop = parseFloat(style.paddingTop);
                    const paddingBottom = parseFloat(style.paddingBottom);
                    const textareaHeight = parseFloat(style.height);
                    const inputAreaRect =
                        document.querySelector(".input-area")!.getBoundingClientRect();

                    const left = rect.left - inputAreaRect.left + cursorCoords.cursorLeft;

                    if (placement === "top") {
                        const top =
                            rect.top +
                            rect.height +
                            Math.min(textareaHeight, cursorCoords.cursorTop) -
                            paddingTop -
                            paddingBottom;
                        setCursorPosition({ bottom: 0, left, top });
                    } else {
                        const bottom =
                            inputAreaRect.top -
                            rect.top -
                            cursorCoords.cursorTop +
                            10 +
                            (textarea.scrollHeight - textarea.clientHeight);
                        setCursorPosition({ bottom, left, top: 0 });
                    }
                },
                [placement]
            );

            const acpSlashNamespaces = useCallback((): SlashNamespaceItem[] => {
                return acpAvailableCommands.map((command) => ({
                    name: command.name.startsWith("/") ? command.name.slice(1) : command.name,
                    description: command.description || "ACP 命令",
                    isEnabled: true,
                    source: "acp",
                    inputHint: command.input_hint ?? null,
                }));
            }, [acpAvailableCommands]);

            const filterSlashNamespaces = useCallback((query: string) => {
                const queryLower = query.trim().toLowerCase();
                const allNamespaces = [
                    ...DEFAULT_SLASH_NAMESPACES,
                    ...acpSlashNamespaces(),
                ];
                if (!queryLower) {
                    return allNamespaces;
                }
                return allNamespaces.filter((namespace) => {
                    return (
                        namespace.name.toLowerCase().includes(queryLower) ||
                        namespace.description.toLowerCase().includes(queryLower)
                    );
                });
            }, [acpSlashNamespaces]);

            const loadBangList = useCallback(async () => {
                try {
                    const bangList = await invoke<BangCompletionItem[]>("get_bang_list");
                    setBangList(bangList);
                    setOriginalBangList(bangList);
                } catch (error) {
                    console.error("[InputArea] Failed to load bang list:", error);
                    setBangList([]);
                    setOriginalBangList([]);
                }
            }, []);

            const loadSlashSkills = useCallback(
                async (forceRefresh = false) => {
                    try {
                        const skills = await invoke<SlashSkillCompletionItem[]>(
                            "get_skills_for_slash_completion",
                            { forceRefresh },
                        );
                        setSlashSkills(skills);
                        setFilteredSlashSkills(
                            PinyinFilter.filterSlashSkills(skills, ""),
                        );
                    } catch (error) {
                        console.warn(
                            "Failed to load skills for slash completion:",
                            error,
                        );
                    }
                },
                [],
            );

            const hideSlashCompletion = useCallback(() => {
                setSlashListVisible(false);
                setSlashContext(null);
            }, []);

            const showSlashCompletion = useCallback(
                (
                    context: SlashCompletionContext,
                    textarea: HTMLTextAreaElement,
                    cursorPosition: number,
                ) => {
                    setSlashContext(context);
                    setSelectedSlashIndex(0);
                    setBangListVisible(false);
                    setAssistantListVisible(false);
                    setArtifactListVisible(false);

                    if (context.kind === "namespace") {
                        const filtered = filterSlashNamespaces(context.query);
                        if (filtered.length === 0) {
                            hideSlashCompletion();
                            return;
                        }
                        setSlashMode("namespace");
                        setFilteredSlashNamespaces(filtered);
                        setSlashListVisible(true);
                    } else {
                        const filtered = PinyinFilter.filterSlashSkills(
                            slashSkills,
                            context.query.replace(/\\([()\\])/g, "$1"),
                        );
                        if (filtered.length === 0) {
                            hideSlashCompletion();
                            return;
                        }
                        setSlashMode("skill");
                        setFilteredSlashSkills(filtered);
                        setSlashListVisible(true);
                    }

                    updateCompletionCursorPosition(textarea, cursorPosition);
                },
                [
                    filterSlashNamespaces,
                    hideSlashCompletion,
                    slashSkills,
                    updateCompletionCursorPosition,
                ],
            );

            const handleSlashNamespaceSelect = useCallback(
                (namespace: SlashNamespaceItem) => {
                    if (
                        !textareaRef.current ||
                        !namespace.isEnabled ||
                        !slashContext ||
                        slashContext.kind !== "namespace"
                    ) {
                        return;
                    }

                    const textarea = textareaRef.current;
                    const beforeSlash = textarea.value.substring(
                        0,
                        slashContext.replaceStart,
                    );
                    const afterSlash = textarea.value.substring(
                        slashContext.replaceEnd,
                    );
                    const completionText =
                        namespace.source === "acp"
                            ? `/${namespace.name}${namespace.inputHint ? " " : ""}`
                            : `/${namespace.name}()`;
                    const newValue = beforeSlash + completionText + afterSlash;
                    const newPosition =
                        namespace.source === "acp"
                            ? beforeSlash.length + completionText.length
                            : beforeSlash.length + completionText.length - 1;

                    setInputText(newValue);

                    setTimeout(() => {
                        textarea.focus();
                        textarea.setSelectionRange(newPosition, newPosition);
                        const nextContext = getSlashCompletionContext(
                            newValue,
                            newPosition,
                        );

                        if (namespace.source !== "acp" && nextContext?.kind === "skill") {
                            showSlashCompletion(
                                nextContext,
                                textarea,
                                newPosition,
                            );
                        } else {
                            hideSlashCompletion();
                        }
                    }, 0);
                },
                [hideSlashCompletion, setInputText, showSlashCompletion, slashContext],
            );

            const handleSlashSkillSelect = useCallback(
                (skill: FilteredSlashSkill) => {
                    if (
                        !textareaRef.current ||
                        !slashContext ||
                        slashContext.kind !== "skill"
                    ) {
                        return;
                    }

                    const textarea = textareaRef.current;
                    const beforeSlash = textarea.value.substring(
                        0,
                        slashContext.replaceStart,
                    );
                    const afterSlash = textarea.value.substring(
                        slashContext.replaceEnd,
                    );
                    const completionText = buildSkillInvocation(skill.invokeName);
                    const newValue = beforeSlash + completionText + afterSlash;
                    const newPosition = beforeSlash.length + completionText.length;

                    setInputText(newValue);
                    hideSlashCompletion();

                    setTimeout(() => {
                        textarea.focus();
                        textarea.setSelectionRange(newPosition, newPosition);
                    }, 0);
                },
                [hideSlashCompletion, setInputText, slashContext],
            );

            // 胶囊高度由 JS 内联写入，任何影响高度的输入（正文/附件/引用条/聚焦）变化都要重算：
            // 聚焦时按内容撑高，未聚焦时回到初始高度。未聚焦也必须同步——点发送钮会先失焦
            // （按“有附件”的高度收起），随后附件被清空，若不重算胶囊会停在旧高度变成圆角矩形。
            // 移动端失焦（点发送钮）后内容被清空也需要回缩，因此始终按内容调整
            const hasTopChip = !!topChip;
            useEffect(() => {
                if (textareaRef.current && !initialHeight) {
                    setInitialHeight(textareaRef.current.scrollHeight);
                }
                if (isFocused || isMobile) {
                    adjustTextareaHeight();
                } else {
                    restoreInitialHeight();
                }
                // eslint-disable-next-line react-hooks/exhaustive-deps
            }, [inputText, initialHeight, fileInfoList, hasTopChip, isFocused, isMobile]);

            useEffect(() => {
                if (!isMobile || !window.visualViewport) return;
                const viewport = window.visualViewport;
                const onResize = () => adjustTextareaHeight();
                viewport.addEventListener("resize", onResize);
                viewport.addEventListener("scroll", onResize);
                return () => {
                    viewport.removeEventListener("resize", onResize);
                    viewport.removeEventListener("scroll", onResize);
                };
            }, [isMobile]);

            useEffect(() => {
                if (!slashListVisible || !slashContext) {
                    return;
                }

                if (slashContext.kind === "namespace") {
                    setFilteredSlashNamespaces(
                        filterSlashNamespaces(slashContext.query),
                    );
                    return;
                }

                setFilteredSlashSkills(
                    PinyinFilter.filterSlashSkills(
                        slashSkills,
                        slashContext.query.replace(/\\([()\\])/g, "$1"),
                    ),
                );
            }, [
                filterSlashNamespaces,
                slashContext,
                slashListVisible,
                slashSkills,
                pinyinReady,
            ]);

            useEffect(() => {
                void loadBangList();
                void loadSlashSkills();

                // Load assistants for @ selection（过滤结果在输入 @ 时才计算，这里只更新候选源）
                invoke<AssistantItem[]>("get_assistants").then(setAssistants);

                // Load artifacts for # selection
                const loadArtifacts = () => {
                    invoke<ArtifactCollectionItem[]>("get_artifacts_for_completion").then((artifactList) => {
                        setArtifacts(artifactList);
                        // Initialize with default match info for all artifacts
                        const initialFiltered: FilteredArtifact[] = artifactList.map((artifact) => ({
                            ...artifact,
                            matchType: "exact" as const,
                            highlightIndices: [],
                        }));
                        setFilteredArtifacts(initialFiltered);
                    });
                };

                loadArtifacts();

                // Listen for artifact collection updates
                const setupArtifactListener = async () => {
                    const unlisteners = await Promise.all([
                        listen("artifact-collection-updated", loadArtifacts),
                        listen("artifacts_collection_changed", loadArtifacts),
                    ]);
                    return () => unlisteners.forEach((unlisten) => unlisten());
                };

                const setupSlashListener = async () => {
                    const unlisten = await listen("skills-registry-changed", () => {
                        void loadSlashSkills(true);
                    });
                    return unlisten;
                };

                const setupPluginListener = async () => {
                    const unlisten = await listen("plugin_registry_changed", () => {
                        void loadBangList();
                    });
                    return unlisten;
                };

                let unlistenPromise: Promise<() => void> | null = null;
                let slashUnlistenPromise: Promise<() => void> | null = null;
                let pluginUnlistenPromise: Promise<() => void> | null = null;

                try {
                    unlistenPromise = setupArtifactListener();
                    slashUnlistenPromise = setupSlashListener();
                    pluginUnlistenPromise = setupPluginListener();
                } catch (error) {
                    console.warn("Failed to setup artifact listener:", error);
                }

                return () => {
                    if (unlistenPromise) {
                        unlistenPromise.then((unlisten) => unlisten()).catch(console.warn);
                    }
                    if (slashUnlistenPromise) {
                        slashUnlistenPromise
                            .then((unlisten) => unlisten())
                            .catch(console.warn);
                    }
                    if (pluginUnlistenPromise) {
                        pluginUnlistenPromise
                            .then((unlisten) => unlisten())
                            .catch(console.warn);
                    }
                };
            }, [loadBangList, loadSlashSkills]);

            // 监听助手列表变化
            useAssistantListListener({
                onAssistantListChanged: useCallback((assistantList: AssistantItem[]) => {
                    setAssistants(assistantList);
                }, []),
            });

            useEffect(() => {
                const handleSelectionChange = () => {
                    if (textareaRef.current) {
                        const cursorPosition = textareaRef.current.selectionStart;
                        const value = textareaRef.current.value;

                        // Handle @ symbol detection
                        const atIndex = value.lastIndexOf("@", cursorPosition - 1);
                        const bangIndex = Math.max(
                            value.lastIndexOf("!", cursorPosition - 1),
                            value.lastIndexOf("！", cursorPosition - 1)
                        );
                        const hashIndex = value.lastIndexOf("#", cursorPosition - 1);
                        const currentSlashContext = getSlashCompletionContext(
                            value,
                            cursorPosition,
                        );
                        const slashIndex =
                            currentSlashContext?.triggerStart ?? -1;

                        // Find the most recent symbol
                        const maxIndex = Math.max(
                            atIndex,
                            bangIndex,
                            hashIndex,
                            slashIndex,
                        );

                        if (
                            currentSlashContext &&
                            slashIndex !== -1 &&
                            slashIndex === maxIndex
                        ) {
                            showSlashCompletion(
                                currentSlashContext,
                                textareaRef.current,
                                cursorPosition,
                            );
                        } else if (hashIndex !== -1 && hashIndex === maxIndex) {
                            // Handle # symbol for artifacts
                            const hashInput = value.substring(hashIndex + 1, cursorPosition).toLowerCase();

                            // Filter artifacts using pinyin
                            const filtered = PinyinFilter.filterArtifacts(artifacts, hashInput);

                            if (filtered.length > 0) {
                                setFilteredArtifacts(filtered);
                                setSelectedArtifactIndex(0);
                                setArtifactListVisible(true);
                                setBangListVisible(false);
                                setAssistantListVisible(false);
                                hideSlashCompletion();

                                const cursorCoords = getCaretCoordinates(textareaRef.current, hashIndex + 1);
                                const rect = textareaRef.current.getBoundingClientRect();
                                const style = window.getComputedStyle(textareaRef.current);
                                const paddingTop = parseFloat(style.paddingTop);
                                const paddingBottom = parseFloat(style.paddingBottom);
                                const textareaHeight = parseFloat(style.height);

                                const inputAreaRect = document.querySelector(".input-area")!.getBoundingClientRect();
                                const left = rect.left - inputAreaRect.left + cursorCoords.cursorLeft;

                                if (placement === "top") {
                                    const top =
                                        rect.top +
                                        rect.height +
                                        Math.min(textareaHeight, cursorCoords.cursorTop) -
                                        paddingTop -
                                        paddingBottom;
                                    setCursorPosition({ bottom: 0, left, top });
                                } else {
                                    const bottom =
                                        inputAreaRect.top -
                                        rect.top -
                                        cursorCoords.cursorTop +
                                        10 +
                                        (textareaRef.current.scrollHeight - textareaRef.current.clientHeight);
                                    setCursorPosition({ bottom, left, top: 0 });
                                }
                            } else {
                                setArtifactListVisible(false);
                                hideSlashCompletion();
                            }
                        } else if (atIndex !== -1 && atIndex === maxIndex) {
                            // Handle @ symbol for assistants
                            const atInput = value.substring(atIndex + 1, cursorPosition).toLowerCase();

                            // Filter assistants using pinyin
                            const filtered = PinyinFilter.filterAssistants(mentionSource, atInput);

                            if (filtered.length > 0) {
                                setFilteredAssistants(filtered);
                                setSelectedAssistantIndex(0);
                                setAssistantListVisible(true);
                                setBangListVisible(false);
                                setArtifactListVisible(false);
                                hideSlashCompletion();

                                const cursorCoords = getCaretCoordinates(textareaRef.current, atIndex + 1);
                                const rect = textareaRef.current.getBoundingClientRect();
                                const style = window.getComputedStyle(textareaRef.current);
                                const paddingTop = parseFloat(style.paddingTop);
                                const paddingBottom = parseFloat(style.paddingBottom);
                                const textareaHeight = parseFloat(style.height);

                                const inputAreaRect = document.querySelector(".input-area")!.getBoundingClientRect();
                                const left = rect.left - inputAreaRect.left + cursorCoords.cursorLeft;

                                if (placement === "top") {
                                    const top =
                                        rect.top +
                                        rect.height +
                                        Math.min(textareaHeight, cursorCoords.cursorTop) -
                                        paddingTop -
                                        paddingBottom;
                                    setCursorPosition({ bottom: 0, left, top });
                                } else {
                                    const bottom =
                                        inputAreaRect.top -
                                        rect.top -
                                        cursorCoords.cursorTop +
                                        10 +
                                        (textareaRef.current.scrollHeight - textareaRef.current.clientHeight);
                                    setCursorPosition({ bottom, left, top: 0 });
                                }
                            } else {
                                setAssistantListVisible(false);
                                hideSlashCompletion();
                            }
                        } else if (bangIndex !== -1 && bangIndex === maxIndex) {
                            // Handle ! symbol (existing logic)
                            const bangInput = value.substring(bangIndex + 1, cursorPosition).toLowerCase();
                            const filteredBangs = originalBangList.filter(([bang]) =>
                                bang.toLowerCase().startsWith(bangInput)
                            );

                            if (filteredBangs.length > 0) {
                                setBangList(filteredBangs);
                                setSelectedBangIndex(0);
                                setBangListVisible(true);
                                setAssistantListVisible(false);
                                setArtifactListVisible(false);
                                hideSlashCompletion();

                                const cursorCoords = getCaretCoordinates(textareaRef.current, bangIndex + 1);
                                const rect = textareaRef.current.getBoundingClientRect();
                                const style = window.getComputedStyle(textareaRef.current);
                                const paddingTop = parseFloat(style.paddingTop);
                                const paddingBottom = parseFloat(style.paddingBottom);
                                const textareaHeight = parseFloat(style.height);

                                const inputAreaRect = document.querySelector(".input-area")!.getBoundingClientRect();
                                const left = rect.left - inputAreaRect.left + cursorCoords.cursorLeft;

                                if (placement === "top") {
                                    const top =
                                        rect.top +
                                        rect.height +
                                        Math.min(textareaHeight, cursorCoords.cursorTop) -
                                        paddingTop -
                                        paddingBottom;
                                    setCursorPosition({ bottom: 0, left, top });
                                } else {
                                    const bottom =
                                        inputAreaRect.top -
                                        rect.top -
                                        cursorCoords.cursorTop +
                                        10 +
                                        (textareaRef.current.scrollHeight - textareaRef.current.clientHeight);
                                    setCursorPosition({ bottom, left, top: 0 });
                                }
                            } else {
                                setBangListVisible(false);
                                hideSlashCompletion();
                            }
                        } else {
                            // Hide all lists when no trigger symbol is active
                            setBangListVisible(false);
                            setAssistantListVisible(false);
                            setArtifactListVisible(false);
                            hideSlashCompletion();
                        }
                    }
                };

                document.addEventListener("selectionchange", handleSelectionChange);
                return () => {
                    document.removeEventListener("selectionchange", handleSelectionChange);
                };
            }, [
                originalBangList,
                mentionSource,
                artifacts,
                placement,
                hideSlashCompletion,
                showSlashCompletion,
            ]);

            // 移动端：textarea 随内容增高（44px 起，约 30vh 封顶），胶囊由 CSS 底边锚定向上生长；
            // 把增量写到根节点 CSS 变量，底部栏同步撑高，消息列表（flex 兄弟）随之收缩而非被遮挡。
            const adjustMobileTextareaHeight = () => {
                const textarea = textareaRef.current;
                if (!textarea) return;
                textarea.style.height = "auto";
                const viewportHeight = window.visualViewport?.height ?? window.innerHeight;
                const maxHeight = mobileTextareaMaxHeight(window.innerHeight, viewportHeight);
                const newHeight = Math.min(
                    Math.max(textarea.scrollHeight, MOBILE_TEXTAREA_MIN_HEIGHT),
                    maxHeight,
                );
                textarea.style.height = `${newHeight}px`;
                rootRef.current?.style.setProperty(
                    COMPOSER_GROW_VAR,
                    `${newHeight - MOBILE_TEXTAREA_MIN_HEIGHT}px`,
                );
            };

            const adjustTextareaHeight = () => {
                if (isMobile) {
                    adjustMobileTextareaHeight();
                    return;
                }
                const textarea = textareaRef.current;
                if (textarea && initialHeight) {
                    textarea.style.height = `${initialHeight}px`;
                    const maxHeight = document.documentElement.clientHeight * 0.35;
                    const newHeight = Math.min(Math.max(textarea.scrollHeight, initialHeight), maxHeight);
                    textarea.style.height = `${newHeight}px`;
                    const filePad = (fileInfoList?.length ? IMAGE_AREA_HEIGHT : 0) + (topChip ? TOP_CHIP_HEIGHT : 0);
                    textarea.parentElement!.style.height = `${newHeight + filePad}px`;
                }
            };

            const restoreInitialHeight = () => {
                if (isMobile) {
                    // 移动端失焦保留内容高度，避免已输入的多行内容被压回一行
                    adjustMobileTextareaHeight();
                    return;
                }
                const textarea = textareaRef.current;
                if (textarea && initialHeight) {
                    textarea.style.height = `${initialHeight}px`;
                    const filePad = (fileInfoList?.length ? IMAGE_AREA_HEIGHT : 0) + (topChip ? TOP_CHIP_HEIGHT : 0);
                    textarea.parentElement!.style.height = `${initialHeight + filePad}px`;
                }
            };

            const handleTextareaChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
                const newValue = e.target.value;
                const cursorPosition = e.target.selectionStart;
                setInputText(newValue);

                // Check for symbols
                const atIndex = newValue.lastIndexOf("@", cursorPosition - 1);
                const bangIndex = Math.max(
                    newValue.lastIndexOf("!", cursorPosition - 1),
                    newValue.lastIndexOf("！", cursorPosition - 1)
                );
                const hashIndex = newValue.lastIndexOf("#", cursorPosition - 1);
                const currentSlashContext = getSlashCompletionContext(
                    newValue,
                    cursorPosition,
                );
                const slashIndex = currentSlashContext?.triggerStart ?? -1;

                // Find the most recent symbol
                const maxIndex = Math.max(
                    atIndex,
                    bangIndex,
                    hashIndex,
                    slashIndex,
                );

                if (
                    currentSlashContext &&
                    slashIndex !== -1 &&
                    slashIndex === maxIndex
                ) {
                    showSlashCompletion(
                        currentSlashContext,
                        e.target,
                        cursorPosition,
                    );
                } else if (hashIndex !== -1 && hashIndex === maxIndex) {
                    // Handle # symbol for artifacts
                    const hashInput = newValue.substring(hashIndex + 1, cursorPosition).toLowerCase();

                    // Filter artifacts using pinyin
                    const filtered = PinyinFilter.filterArtifacts(artifacts, hashInput);

                    if (filtered.length > 0) {
                        setFilteredArtifacts(filtered);
                        setSelectedArtifactIndex(0);
                        setArtifactListVisible(true);
                        setBangListVisible(false);
                        setAssistantListVisible(false);
                        hideSlashCompletion();

                        // Update cursor position
                        const textarea = e.target;
                        const cursorCoords = getCaretCoordinates(textarea, cursorPosition);
                        const rect = textarea.getBoundingClientRect();
                        const style = window.getComputedStyle(textarea);
                        const paddingTop = parseFloat(style.paddingTop);
                        const paddingBottom = parseFloat(style.paddingBottom);
                        const textareaHeight = parseFloat(style.height);
                        const inputAreaRect = document.querySelector(".input-area")!.getBoundingClientRect();

                        const left = rect.left - inputAreaRect.left + cursorCoords.cursorLeft;

                        if (placement === "top") {
                            const top =
                                rect.top +
                                rect.height +
                                Math.min(textareaHeight, cursorCoords.cursorTop) -
                                paddingTop -
                                paddingBottom;

                            setCursorPosition({ bottom: 0, left, top });
                        } else {
                            const bottom =
                                inputAreaRect.top -
                                rect.top -
                                cursorCoords.cursorTop +
                                10 +
                                (textarea.scrollHeight - textarea.clientHeight);
                            setCursorPosition({ bottom, left, top: 0 });
                        }
                    } else {
                        setArtifactListVisible(false);
                        hideSlashCompletion();
                    }
                } else if (atIndex !== -1 && atIndex === maxIndex) {
                    // Handle @ symbol for assistants
                    const atInput = newValue.substring(atIndex + 1, cursorPosition).toLowerCase();

                    // Filter assistants using pinyin
                    const filtered = PinyinFilter.filterAssistants(mentionSource, atInput);

                    if (filtered.length > 0) {
                        setFilteredAssistants(filtered);
                        setSelectedAssistantIndex(0);
                        setAssistantListVisible(true);
                        setBangListVisible(false);
                        setArtifactListVisible(false);
                        hideSlashCompletion();

                        // Update cursor position
                        const textarea = e.target;
                        const cursorCoords = getCaretCoordinates(textarea, cursorPosition);
                        const rect = textarea.getBoundingClientRect();
                        const style = window.getComputedStyle(textarea);
                        const paddingTop = parseFloat(style.paddingTop);
                        const paddingBottom = parseFloat(style.paddingBottom);
                        const textareaHeight = parseFloat(style.height);
                        const inputAreaRect = document.querySelector(".input-area")!.getBoundingClientRect();

                        const left = rect.left - inputAreaRect.left + cursorCoords.cursorLeft;

                        if (placement === "top") {
                            const top =
                                rect.top +
                                rect.height +
                                Math.min(textareaHeight, cursorCoords.cursorTop) -
                                paddingTop -
                                paddingBottom;

                            setCursorPosition({ bottom: 0, left, top });
                        } else {
                            const bottom =
                                inputAreaRect.top -
                                rect.top -
                                cursorCoords.cursorTop +
                                10 +
                                (textarea.scrollHeight - textarea.clientHeight);
                            setCursorPosition({ bottom, left, top: 0 });
                        }
                    } else {
                        setAssistantListVisible(false);
                        hideSlashCompletion();
                    }
                } else if (bangIndex !== -1 && bangIndex === maxIndex) {
                    // Handle ! symbol (existing logic)
                    const bangInput = newValue.substring(bangIndex + 1, cursorPosition).toLowerCase();
                    const filteredBangs = originalBangList.filter(([bang]) => bang.toLowerCase().startsWith(bangInput));

                    if (filteredBangs.length > 0) {
                        setBangList(filteredBangs);
                        setSelectedBangIndex(0);
                        setBangListVisible(true);
                        setAssistantListVisible(false);
                        setArtifactListVisible(false);
                        hideSlashCompletion();

                        // Update cursor position
                        const textarea = e.target;
                        const cursorCoords = getCaretCoordinates(textarea, cursorPosition);
                        const rect = textarea.getBoundingClientRect();
                        const style = window.getComputedStyle(textarea);
                        const paddingTop = parseFloat(style.paddingTop);
                        const paddingBottom = parseFloat(style.paddingBottom);
                        const textareaHeight = parseFloat(style.height);
                        const inputAreaRect = document.querySelector(".input-area")!.getBoundingClientRect();

                        const left = rect.left - inputAreaRect.left + cursorCoords.cursorLeft;

                        if (placement === "top") {
                            const top =
                                rect.top +
                                rect.height +
                                Math.min(textareaHeight, cursorCoords.cursorTop) -
                                paddingTop -
                                paddingBottom;

                            setCursorPosition({ bottom: 0, left, top });
                        } else {
                            const bottom =
                                inputAreaRect.top -
                                rect.top -
                                cursorCoords.cursorTop +
                                10 +
                                (textarea.scrollHeight - textarea.clientHeight);
                            setCursorPosition({ bottom, left, top: 0 });
                        }
                    } else {
                        setBangListVisible(false);
                        hideSlashCompletion();
                    }
                } else {
                    // Hide all lists when no trigger symbol is active
                    setBangListVisible(false);
                    setAssistantListVisible(false);
                    setArtifactListVisible(false);
                    hideSlashCompletion();
                }
            };

            const handleKeyDownWithBang = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
                // WebKit2 GTK 中文输入法兼容性：双重检查 IME 状态
                // 1. !isComposingRef.current: 手动跟踪的状态（更可靠）
                // 2. !e.nativeEvent.isComposing: 原生 API 状态（作为补充）
                // 只有两个条件都满足时才认为不在 IME 组合状态，可以安全地处理回车键
                const isEnterPressed = e.key === "Enter" && !isComposingRef.current && !e.nativeEvent.isComposing;

                if (isEnterPressed) {
                    if (e.shiftKey) {
                        // Shift + Enter for new line
                        return;
                    } else if (slashListVisible) {
                        e.preventDefault();
                        if (slashMode === "namespace") {
                            const selectedNamespace =
                                filteredSlashNamespaces[selectedSlashIndex];
                            if (selectedNamespace?.isEnabled) {
                                handleSlashNamespaceSelect(selectedNamespace);
                            }
                        } else {
                            const selectedSkill =
                                filteredSlashSkills[selectedSlashIndex];
                            if (selectedSkill) {
                                handleSlashSkillSelect(selectedSkill);
                            }
                        }
                    } else if (artifactListVisible) {
                        // Open artifact - 阻止表单提交
                        e.preventDefault();
                        const selectedArtifact = filteredArtifacts[selectedArtifactIndex];
                        const textarea = e.currentTarget as HTMLTextAreaElement;
                        const cursorPosition = textarea.selectionStart;
                        const hashIndex = textarea.value.lastIndexOf("#", cursorPosition - 1);

                        if (hashIndex !== -1) {
                            // Clear the #artifact_name from input
                            const beforeHash = textarea.value.substring(0, hashIndex);
                            const afterHash = textarea.value.substring(cursorPosition);
                            setInputText(beforeHash + afterHash);

                            // Set cursor position at the hash position
                            setTimeout(() => {
                                textarea.setSelectionRange(hashIndex, hashIndex);
                            }, 0);
                        }
                        setArtifactListVisible(false);

                        // Open the artifact
                        void openArtifact(selectedArtifact.id);
                    } else if (assistantListVisible) {
                        // Select assistant - 阻止表单提交
                        e.preventDefault();
                        const selectedAssistant = filteredAssistants[selectedAssistantIndex];
                        const textarea = e.currentTarget as HTMLTextAreaElement;
                        const cursorPosition = textarea.selectionStart;
                        const atIndex = textarea.value.lastIndexOf("@", cursorPosition - 1);

                        if (atIndex !== -1) {
                            const beforeAt = textarea.value.substring(0, atIndex);
                            const afterAt = textarea.value.substring(cursorPosition);
                            setInputText(beforeAt + `@${selectedAssistant.name} ` + afterAt);

                            // 设置光标位置
                            setTimeout(() => {
                                const newPosition = atIndex + selectedAssistant.name.length + 2;
                                textarea.setSelectionRange(newPosition, newPosition);
                            }, 0);
                        }
                        setAssistantListVisible(false);
                    } else if (bangListVisible) {
                        // Select bang - 阻止表单提交
                        e.preventDefault();
                        const selectedBang = bangList[selectedBangIndex];
                        let complete = selectedBang[1];
                        const textarea = e.currentTarget as HTMLTextAreaElement;
                        const cursorPosition = textarea.selectionStart;
                        const bangIndex = Math.max(
                            textarea.value.lastIndexOf("!", cursorPosition - 1),
                            textarea.value.lastIndexOf("！", cursorPosition - 1)
                        );

                        if (bangIndex !== -1) {
                            // 找到complete中的|的位置
                            const cursorIndex = complete.indexOf("|");
                            // 如果有|，则将光标移动到|的位置，并且移除|
                            if (cursorIndex !== -1) {
                                complete = complete.substring(0, cursorIndex) + complete.substring(cursorIndex + 1);
                            }

                            const beforeBang = textarea.value.substring(0, bangIndex);
                            const afterBang = textarea.value.substring(cursorPosition);
                            setInputText(beforeBang + "!" + complete + " " + afterBang);

                            // 设置光标位置
                            setTimeout(() => {
                                const newPosition =
                                    bangIndex + (cursorIndex === -1 ? selectedBang[0].length + 2 : cursorIndex + 1);
                                textarea.setSelectionRange(newPosition, newPosition);
                            }, 0);
                        }
                        setBangListVisible(false);
                    } else if (isMobile) {
                        // 移动端：软键盘回车只换行，发送统一走发送按钮
                        return;
                    } else {
                        // Enter for submit (only if not in IME composition)
                        e.preventDefault();
                        handleSend();
                    }
                } else if (e.key === "Tab" && slashListVisible) {
                    e.preventDefault();
                    if (slashMode === "namespace") {
                        const selectedNamespace =
                            filteredSlashNamespaces[selectedSlashIndex];
                        if (selectedNamespace?.isEnabled) {
                            handleSlashNamespaceSelect(selectedNamespace);
                        }
                    } else {
                        const selectedSkill =
                            filteredSlashSkills[selectedSlashIndex];
                        if (selectedSkill) {
                            handleSlashSkillSelect(selectedSkill);
                        }
                    }
                } else if (e.key === "Tab" && artifactListVisible) {
                    // Select artifact
                    e.preventDefault();
                    const selectedArtifact = filteredArtifacts[selectedArtifactIndex];
                    const textarea = e.currentTarget as HTMLTextAreaElement;
                    const cursorPosition = textarea.selectionStart;
                    const hashIndex = textarea.value.lastIndexOf("#", cursorPosition - 1);

                    if (hashIndex !== -1) {
                        const beforeHash = textarea.value.substring(0, hashIndex);
                        const afterHash = textarea.value.substring(cursorPosition);
                        setInputText(beforeHash + `#${selectedArtifact.name} ` + afterHash);

                        // 设置光标位置
                        setTimeout(() => {
                            const newPosition = hashIndex + selectedArtifact.name.length + 2;
                            textarea.setSelectionRange(newPosition, newPosition);
                        }, 0);
                    }
                    setArtifactListVisible(false);
                } else if (e.key === "Tab" && assistantListVisible) {
                    // Select assistant
                    e.preventDefault();
                    const selectedAssistant = filteredAssistants[selectedAssistantIndex];
                    const textarea = e.currentTarget as HTMLTextAreaElement;
                    const cursorPosition = textarea.selectionStart;
                    const atIndex = textarea.value.lastIndexOf("@", cursorPosition - 1);

                    if (atIndex !== -1) {
                        const beforeAt = textarea.value.substring(0, atIndex);
                        const afterAt = textarea.value.substring(cursorPosition);
                        setInputText(beforeAt + `@${selectedAssistant.name} ` + afterAt);

                        // 设置光标位置
                        setTimeout(() => {
                            const newPosition = atIndex + selectedAssistant.name.length + 2;
                            textarea.setSelectionRange(newPosition, newPosition);
                        }, 0);
                    }
                    setAssistantListVisible(false);
                } else if (e.key === "Tab" && bangListVisible) {
                    // Select bang
                    e.preventDefault();
                    const selectedBang = bangList[selectedBangIndex];
                    let complete = selectedBang[1];
                    const textarea = e.currentTarget as HTMLTextAreaElement;
                    const cursorPosition = textarea.selectionStart;
                    const bangIndex = Math.max(
                        textarea.value.lastIndexOf("!", cursorPosition - 1),
                        textarea.value.lastIndexOf("！", cursorPosition - 1)
                    );

                    if (bangIndex !== -1) {
                        // 找到complete中的|的位置
                        const cursorIndex = complete.indexOf("|");
                        // 如果有|，则将光标移动到|的位置，并且移除|
                        if (cursorIndex !== -1) {
                            complete = complete.substring(0, cursorIndex) + complete.substring(cursorIndex + 1);
                        }

                        const beforeBang = textarea.value.substring(0, bangIndex);
                        const afterBang = textarea.value.substring(cursorPosition);
                        setInputText(beforeBang + "!" + complete + " " + afterBang);

                        // 设置光标位置
                        setTimeout(() => {
                            const newPosition =
                                bangIndex + (cursorIndex === -1 ? selectedBang[0].length + 2 : cursorIndex + 1);
                            textarea.setSelectionRange(newPosition, newPosition);
                        }, 0);
                    }
                    setBangListVisible(false);
                } else if (e.key === "ArrowUp" && slashListVisible) {
                    e.preventDefault();
                    const itemsCount =
                        slashMode === "namespace"
                            ? filteredSlashNamespaces.length
                            : filteredSlashSkills.length;
                    setSelectedSlashIndex((prevIndex) =>
                        prevIndex > 0 ? prevIndex - 1 : itemsCount - 1,
                    );
                } else if (e.key === "ArrowDown" && slashListVisible) {
                    e.preventDefault();
                    const itemsCount =
                        slashMode === "namespace"
                            ? filteredSlashNamespaces.length
                            : filteredSlashSkills.length;
                    setSelectedSlashIndex((prevIndex) =>
                        prevIndex < itemsCount - 1 ? prevIndex + 1 : 0,
                    );
                } else if (e.key === "ArrowUp" && artifactListVisible) {
                    e.preventDefault();
                    setSelectedArtifactIndex((prevIndex) =>
                        prevIndex > 0 ? prevIndex - 1 : filteredArtifacts.length - 1
                    );
                } else if (e.key === "ArrowDown" && artifactListVisible) {
                    e.preventDefault();
                    setSelectedArtifactIndex((prevIndex) =>
                        prevIndex < filteredArtifacts.length - 1 ? prevIndex + 1 : 0
                    );
                } else if (e.key === "ArrowUp" && assistantListVisible) {
                    e.preventDefault();
                    setSelectedAssistantIndex((prevIndex) =>
                        prevIndex > 0 ? prevIndex - 1 : filteredAssistants.length - 1
                    );
                } else if (e.key === "ArrowDown" && assistantListVisible) {
                    e.preventDefault();
                    setSelectedAssistantIndex((prevIndex) =>
                        prevIndex < filteredAssistants.length - 1 ? prevIndex + 1 : 0
                    );
                } else if (e.key === "ArrowUp" && bangListVisible) {
                    e.preventDefault();
                    setSelectedBangIndex((prevIndex) => (prevIndex > 0 ? prevIndex - 1 : bangList.length - 1));
                } else if (e.key === "ArrowDown" && bangListVisible) {
                    e.preventDefault();
                    setSelectedBangIndex((prevIndex) => (prevIndex < bangList.length - 1 ? prevIndex + 1 : 0));
                } else if (e.key === "Escape") {
                    e.preventDefault();
                    if (
                        bangListVisible ||
                        assistantListVisible ||
                        artifactListVisible ||
                        slashListVisible
                    ) {
                        // Hide completion lists
                        setBangListVisible(false);
                        setAssistantListVisible(false);
                        setArtifactListVisible(false);
                        hideSlashCompletion();
                    } else {
                        // No completion lists visible, blur the textarea to remove focus
                        textareaRef.current?.blur();
                    }
                }
            };

            function scrollToSelectedBang() {
                const selectedBangElement = document.querySelector(".completion-bang-container.selected");
                if (selectedBangElement) {
                    const parentElement = selectedBangElement.parentElement;
                    if (parentElement) {
                        const parentRect = parentElement.getBoundingClientRect();
                        const selectedRect = selectedBangElement.getBoundingClientRect();

                        if (selectedRect.top < parentRect.top) {
                            parentElement.scrollTop -= parentRect.top - selectedRect.top;
                        } else if (selectedRect.bottom > parentRect.bottom) {
                            parentElement.scrollTop += selectedRect.bottom - parentRect.bottom;
                        }
                    }
                }
            }
            useEffect(() => {
                scrollToSelectedBang();
            }, [selectedBangIndex]);

            const handleImageContainerClick = useCallback(() => {
                textareaRef.current?.focus();
            }, [textareaRef]);

            const showPlanModeToggle = Boolean(onPlanModeToggle) && placement === "bottom";
            const planToggleDisabled = planModeSwitching || aiIsResponsing;
            const planToggleTitle = aiIsResponsing
                ? "回复进行中，结束后可切换"
                : planMode
                    ? "已开启：Agent 先出方案，你确认后再执行。点击关闭"
                    : "开启后 Agent 先出方案，你确认后再执行";
            const addButtonRight = (sidebarVisible ? 150 : 190) + sidebarWidth;
            const baseRight = sidebarVisible ? 130 : 170;
            const defaultSendButtonIcon = aiIsResponsing && !inputText.trim() ? (
                <Square size={20} className="text-action-foreground" />
            ) : (
                <ArrowUp size={20} className="text-action-foreground" />
            );

            const attachmentCount = fileInfoList?.length ?? 0;
            const hasAttachments = attachmentCount > 0;

            return (
                <div
                    ref={rootRef}
                    className={`input-area ${placement} ${isMobile ? 'mobile' : ''} ${showPlanModeToggle ? 'has-footer' : ''} ${isMobile && hasAttachments ? 'has-attachments' : ''}`}
                    data-theme-slot="input-area"
                    data-aipp-slot="chat-input-area"
                    data-attachment-count={attachmentCount}
                >
                    {isMobile && hasAttachments ? (
                        <div
                            className="input-area-attachment-tray"
                            data-aipp-slot="chat-input-attachment-tray"
                            onClick={handleImageContainerClick}
                        >
                            <div className="input-area-attachment-tray-header">
                                <span data-aipp-slot="chat-input-attachment-count">附件 {attachmentCount}</span>
                            </div>
                            <div className="input-area-img-container" data-aipp-slot="chat-input-attachments">
                                {renderFiles()}
                            </div>
                        </div>
                    ) : null}
                    <div
                        className="input-area-textarea-container"
                        style={isMobile ? undefined : { right: baseRight }}
                        data-theme-slot="input-area-container"
                        data-aipp-slot="chat-input-area-container"
                    >
                        {!isMobile && topChip ? (
                            <div className="flex min-w-0 items-center pt-1.5" style={{ height: TOP_CHIP_HEIGHT }} data-aipp-slot="chat-input-top-chip">
                                {topChip}
                            </div>
                        ) : null}
                        {!isMobile ? (
                            <div className="input-area-img-container" onClick={handleImageContainerClick} data-aipp-slot="chat-input-attachments">
                                {renderFiles()}
                            </div>
                        ) : null}
                        <textarea
                            ref={textareaRef}
                            className="input-area-textarea"
                            data-theme-slot="input-area-textarea"
                            data-aipp-slot="chat-input-textarea"
                            rows={1}
                            autoCapitalize="none"
                            enterKeyHint={isMobile ? "enter" : undefined}
                            placeholder={placeholder}
                            aria-label={ariaLabel}
                            value={inputText}
                            onChange={handleTextareaChange}
                            onKeyDown={handleKeyDownWithBang}
                            onPaste={handlePaste}
                            onFocus={() => {
                                setIsFocused(true);
                                adjustTextareaHeight();
                            }}
                            onBlur={() => {
                                setIsFocused(false);
                                restoreInitialHeight();
                            }}
                            onCompositionStart={() => {
                                // WebKit2 GTK 中文输入法兼容性：标记开始 IME 组合
                                isComposingRef.current = true;
                            }}
                            onCompositionEnd={() => {
                                // WebKit2 GTK 中文输入法兼容性：延迟标记结束 IME 组合
                                // 使用 100ms 延迟是因为 WebKit2 的事件时序不稳定
                                // 确保键盘事件处理器能正确检测到组合状态
                                setTimeout(() => {
                                    isComposingRef.current = false;
                                }, 100);
                            }}
                        />
                    </div>

                    {showPlanModeToggle ? (
                        <div
                            className="input-area-footer"
                            data-theme-slot="input-area-footer"
                            data-aipp-slot="chat-input-footer"
                        >
                            <div
                                className="input-area-plan-mode"
                                data-theme-slot="input-area-plan-mode"
                                data-aipp-slot="chat-input-plan-mode"
                                data-state={planMode ? "on" : "off"}
                                title={planToggleTitle}
                            >
                                <Switch
                                    id={planSwitchId}
                                    checked={planMode}
                                    onCheckedChange={() => onPlanModeToggle?.()}
                                    disabled={planToggleDisabled}
                                    aria-label="Plan 模式"
                                />
                                <label htmlFor={planSwitchId} className="input-area-plan-mode-label">
                                    Plan 模式
                                </label>
                                {planModeSwitching ? (
                                    <Loader2 className="input-area-plan-mode-spinner animate-spin" aria-label="正在切换" />
                                ) : null}
                            </div>
                        </div>
                    ) : null}

                    {handleChooseFile ? (
                        <CircleButton
                            onClick={handleChooseFile}
                            icon={<Plus size={16} className="text-icon" />}
                            className={`input-area-add-button ${placement}`}
                            dataThemeSlot="input-area-add-button"
                            dataAippSlot="chat-input-add-button"
                            aria-label="添加附件"
                            style={
                                placement === "bottom" && !isMobile
                                    ? { right: addButtonRight }
                                    : undefined
                            }
                        />
                    ) : null}
                    <CircleButton
                        size={isMobile ? "medium" : placement === "bottom" ? "large" : "medium"}
                        onClick={handleSend}
                        icon={sendButtonVisual ?? sendButtonIcon ?? defaultSendButtonIcon}
                        primary
                        className={`input-area-send-button ${placement} ${sendButtonVisual ? "has-custom-visual" : ""}`}
                        dataThemeSlot="input-area-send-button"
                        dataAippSlot="chat-input-send-button"
                        aria-label={aiIsResponsing && !inputText.trim() ? "停止" : "发送"}
                        dataState={aiIsResponsing ? "responding" : "idle"}
                        customVisual={Boolean(sendButtonVisual)}
                        style={
                            placement === "bottom" && !isMobile
                                ? { right: (sidebarVisible ? 70 : 107) + sidebarWidth }
                                : undefined
                        }
                    />

                    <SlashCompletionList
                        visible={slashListVisible}
                        placement={placement}
                        cursorPosition={cursorPosition}
                        mode={slashMode}
                        namespaces={filteredSlashNamespaces}
                        skills={filteredSlashSkills}
                        selectedIndex={selectedSlashIndex}
                        onSelectNamespace={handleSlashNamespaceSelect}
                        onSelectSkill={handleSlashSkillSelect}
                    />

                    <BangCompletionList
                        bangListVisible={bangListVisible}
                        placement={placement}
                        cursorPosition={cursorPosition}
                        bangList={bangList}
                        selectedBangIndex={selectedBangIndex}
                        textareaRef={textareaRef}
                        setInputText={setInputText}
                        setBangListVisible={setBangListVisible}
                    />

                    <AssistantCompletionList
                        assistantListVisible={assistantListVisible}
                        placement={placement}
                        cursorPosition={cursorPosition}
                        assistants={filteredAssistants}
                        selectedAssistantIndex={selectedAssistantIndex}
                        textareaRef={textareaRef}
                        setInputText={setInputText}
                        setAssistantListVisible={setAssistantListVisible}
                    />

                    <ArtifactCompletionList
                        artifactListVisible={artifactListVisible}
                        placement={placement}
                        cursorPosition={cursorPosition}
                        artifacts={filteredArtifacts}
                        selectedArtifactIndex={selectedArtifactIndex}
                        textareaRef={textareaRef}
                        setInputText={setInputText}
                        setArtifactListVisible={setArtifactListVisible}
                        onArtifactSelect={(artifact, action) => {
                            if (action === "open") {
                                // Open artifact when Enter is pressed
                                void openArtifact(artifact.id);
                            } else {
                                // Tab key completion - just log for now
                                console.log("Artifact completed:", artifact.name);
                            }
                        }}
                    />
                </div>
            );
        }
    )
);

export default InputArea;
