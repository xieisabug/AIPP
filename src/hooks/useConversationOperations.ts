import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { throttle } from "lodash";
import { Message, Conversation, FileInfo } from "../data/Conversation";
import { AssistantListItem } from "../data/Assistant";
import { extractAssistantFromMessage } from "../utils/assistantMentions";
import useConversationManager from "./useConversationManager";

// 从 plugin.d.ts 导入的接口类型
interface AiResponse {
    conversation_id: number;
    request_prompt_result_with_context: string;
}

export interface UseConversationOperationsProps {
    conversation?: Conversation;
    selectedAssistant: number;
    assistants: AssistantListItem[];
    setMessages: React.Dispatch<React.SetStateAction<Message[]>>;
    inputText: string;
    setInputText: React.Dispatch<React.SetStateAction<string>>;
    fileInfoList?: FileInfo[];
    clearFileInfoList: () => void;
    aiIsResponsing: boolean;
    setAiIsResponsing: (isResponsing: boolean) => void;
    onChangeConversationId: (conversationId: string) => void;
    setShiningMessageIds: React.Dispatch<React.SetStateAction<Set<number>>>;
    setManualShineMessage: (messageId: number | null) => void;
    updateShiningMessages: () => void;
    clearShiningMessages: () => void;
    assistantTypePluginMap: Map<number, any>;
    assistantRunApi: any;
}

export interface UseConversationOperationsReturn {
    // 对话管理
    handleDeleteConversationSuccess: () => void;

    // 消息操作
    handleMessageRegenerate: (regenerateMessageId: number) => void;
    handleMessageEdit: (message: Message) => void;
    handleMessageFork: (messageId: number) => void;
    handleEditSave: (content: string) => void;
    handleEditSaveAndRegenerate: (content: string) => void;
    handleSend: () => void;
    handleArtifact: (lang: string, inputStr: string) => void;

    // 编辑对话框状态
    editDialogIsOpen: boolean;
    editingMessage: Message | null;
    closeEditDialog: () => void;

    // 标题编辑
    titleEditDialogIsOpen: boolean;
    openTitleEditDialog: () => void;
    closeTitleEditDialog: () => void;
}

export function useConversationOperations({
    conversation,
    selectedAssistant,
    assistants,
    setMessages,
    inputText,
    setInputText,
    fileInfoList,
    clearFileInfoList,
    aiIsResponsing,
    setAiIsResponsing,
    onChangeConversationId,
    setShiningMessageIds: _setShiningMessageIds,
    setManualShineMessage,
    updateShiningMessages,
    clearShiningMessages,
    assistantTypePluginMap,
    assistantRunApi,
}: UseConversationOperationsProps): UseConversationOperationsReturn {

    // 对话标题管理相关状态
    const [titleEditDialogIsOpen, setTitleEditDialogIsOpen] = useState<boolean>(false);

    // 消息编辑相关状态
    const [editDialogIsOpen, setEditDialogIsOpen] = useState<boolean>(false);
    const [editingMessage, setEditingMessage] = useState<Message | null>(null);

    // 对话管理相关操作
    const handleDeleteConversationSuccess = useCallback(() => {
        // 删除成功后清空会话ID，返回新建对话界面
        onChangeConversationId("");
    }, [onChangeConversationId]);

    // 消息重新生成处理
    const handleMessageRegenerate = useCallback(
        (regenerateMessageId: number) => {
            // 设置AI响应状态
            setAiIsResponsing(true);

            // 点击的消息先闪亮，等待后端 activity_focus 接管
            setManualShineMessage(regenerateMessageId);

            invoke<AiResponse>("regenerate_ai", {
                messageId: regenerateMessageId,
            })
                .then((res) => {
                    console.log("regenerate ai response", res);
                    // 重新生成消息的处理逻辑
                    // setMessageId(res.add_message_id);
                })
                .catch((error) => {
                    console.error("Regenerate error:", error);
                    setAiIsResponsing(false);
                    // 使用智能边框控制，而不是直接清空
                    updateShiningMessages();
                    // 错误信息将在对话框中显示
                });
        },
        [setAiIsResponsing, updateShiningMessages],
    );

    // 消息分支处理函数
    const { forkConversation } = useConversationManager();
    const handleMessageFork = useCallback(
        async (messageId: number) => {
            if (!conversation?.id) {
                toast.error("无法分支对话：当前对话不存在");
                return;
            }

            try {
                const newConversationId = await forkConversation(conversation.id, messageId);
                toast.success("对话分支创建成功");
                // 导航到新对话
                onChangeConversationId(newConversationId.toString());
            } catch (error) {
                console.error("Fork conversation error:", error);
                toast.error("创建对话分支失败: " + error);
            }
        },
        [conversation?.id, forkConversation, onChangeConversationId]
    );

    // 消息编辑相关处理函数
    const handleMessageEdit = useCallback((message: Message) => {
        setEditingMessage(message);
        setEditDialogIsOpen(true);
    }, []);

    const closeEditDialog = useCallback(() => {
        setEditDialogIsOpen(false);
        setEditingMessage(null);
    }, []);

    const handleEditSave = useCallback(
        (content: string) => {
            if (!editingMessage) return;

            invoke("update_message_content", {
                messageId: editingMessage.id,
                content: content,
            })
                .then(() => {
                    // 更新本地消息状态
                    setMessages((prevMessages) =>
                        prevMessages.map((msg) =>
                            msg.id === editingMessage.id
                                ? { ...msg, content: content }
                                : msg,
                        ),
                    );
                    toast.success("消息已更新");
                })
                .catch((error) => {
                    toast.error("更新消息失败: " + error);
                });
        },
        [editingMessage, setMessages],
    );

    const handleEditSaveAndRegenerate = useCallback(
        (content: string) => {
            if (!editingMessage) return;

            // 先更新消息内容
            invoke("update_message_content", {
                messageId: editingMessage.id,
                content: content,
            })
                .then(() => {
                    // 更新本地消息状态
                    setMessages((prevMessages) =>
                        prevMessages.map((msg) =>
                            msg.id === editingMessage.id
                                ? { ...msg, content: content }
                                : msg,
                        ),
                    );

                    // 然后触发重新生成
                    handleMessageRegenerate(editingMessage.id);

                    toast.success("消息已更新并开始重新生成");
                })
                .catch((error) => {
                    toast.error("更新消息失败: " + error);
                });
        },
        [editingMessage, handleMessageRegenerate, setMessages],
    );

    // 代码运行处理
    const handleArtifact = useCallback((lang: string, inputStr: string) => {
        const conversationId = conversation?.id;
        invoke("run_artifacts", { lang, inputStr, conversationId })
            .then((res) => {
                console.log(res);
            })
            .catch((error) => {
                toast.error("运行失败: " + JSON.stringify(error));
            });
    }, [conversation?.id]);

    // 打开标题编辑对话框
    const openTitleEditDialog = useCallback(() => {
        setTitleEditDialogIsOpen(true);
    }, []);

    // 关闭标题编辑对话框
    const closeTitleEditDialog = useCallback(() => {
        setTitleEditDialogIsOpen(false);
    }, []);

    // 发送消息的主要处理函数，使用节流防止频繁点击
    const handleSend = throttle(() => {
        if (aiIsResponsing) {
            // AI正在响应时，点击取消
            console.log("Cancelling AI");
            console.log(conversation?.id);
            invoke("cancel_ai", { conversationId: +(conversation?.id || 0) })
                .then(() => {
                    setAiIsResponsing(false);
                    // 立即清理本地闪烁状态，避免后端事件未到导致残留
                    clearShiningMessages();
                    // 再次触发智能边框更新，确保 UI 与状态一致
                    updateShiningMessages();
                })
                .catch((err) => {
                    console.error("cancel_ai invoke failed:", err);
                    // 即使取消失败，也要确保本地 UI 清理，避免闪烁残留
                    setAiIsResponsing(false);
                    clearShiningMessages();
                    updateShiningMessages();
                });
        } else {
            // 正常发送消息流程
            if (inputText.trim() === "") {
                setInputText("");
                return;
            }
            setAiIsResponsing(true);

            let conversationId = "";
            let assistantId: string;
            if (!conversation || !conversation.id) {
                assistantId = String(selectedAssistant);
            } else {
                conversationId = String(conversation.id);
                assistantId = String(conversation.assistant_id);
            }
            // 解析 @assistant ，不直接修改 props 参数，使用局部最终 prompt 变量
            let finalPrompt = inputText;
            let parsedAssistantId = +assistantId;
            let mentionParsed = false;
            try {
                const parsed = extractAssistantFromMessage(assistants, inputText, +assistantId);
                parsedAssistantId = parsed.assistantId;
                finalPrompt = parsed.cleanedPrompt;
                if (parsedAssistantId !== +assistantId) {
                    assistantId = String(parsedAssistantId);
                    mentionParsed = true;
                }
            } catch (e) {
                console.warn("extractAssistantFromMessage failed, fallback original", e);
            }

            const assistantData = assistants.find((a) => a.id === parsedAssistantId);
            // ACP 助手 (assistant_type === 4) 走后端 ask_ai 路径，不作为插件处理
            const isPluginAssistant = assistantData?.assistant_type !== 0 && assistantData?.assistant_type !== 4;

            if (isPluginAssistant) {
                // 如果是通过 @ 指定的插件助手，但当前选中的助手与其不同，
                // 原始 assistantRunApi.getAssistantId() / getModelId() 仍会返回旧值，需要包装。
                const needWrap = mentionParsed;
                if (needWrap) {
                    // 预取模型ID，避免插件刚进入时拿到旧模型
                    invoke<any>("get_assistant", { assistantId: parsedAssistantId })
                        .then((detail) => {
                            const wrappedModelId: string = detail?.model?.[0]?.model_code ?? "";
                            const runtimeApi = {
                                ...assistantRunApi,
                                getAssistantId: () => String(parsedAssistantId),
                                getUserInput: () => finalPrompt,
                                getModelId: () => wrappedModelId || "",
                                getField: (_assistantId: string, fieldName: string) =>
                                    assistantRunApi.getField(String(parsedAssistantId), fieldName),
                                askAssistant: (options: any) => {
                                    const merged = {
                                        ...options,
                                        assistantId: String(parsedAssistantId),
                                        question: options?.question ?? finalPrompt,
                                    };
                                    return assistantRunApi.askAssistant(merged);
                                },
                            };

                            assistantTypePluginMap
                                .get(assistantData?.assistant_type ?? 0)
                                ?.onAssistantTypeRun(runtimeApi);
                        })
                        .catch((e) => {
                            console.warn("prefetch assistant detail failed, run plugin without model id override", e);
                            const runtimeApi = {
                                ...assistantRunApi,
                                getAssistantId: () => String(parsedAssistantId),
                                getUserInput: () => finalPrompt,
                                getField: (_assistantId: string, fieldName: string) =>
                                    assistantRunApi.getField(String(parsedAssistantId), fieldName),
                                askAssistant: (options: any) => {
                                    const merged = {
                                        ...options,
                                        assistantId: String(parsedAssistantId),
                                        question: options?.question ?? finalPrompt,
                                    };
                                    return assistantRunApi.askAssistant(merged);
                                },
                            };
                            assistantTypePluginMap
                                .get(assistantData?.assistant_type ?? 0)
                                ?.onAssistantTypeRun(runtimeApi);
                        });
                } else {
                    assistantTypePluginMap
                        .get(assistantData?.assistant_type ?? 0)
                        ?.onAssistantTypeRun(assistantRunApi);
                }
            } else {
                console.log("[ACP DEBUG] Calling ask_ai with:", {
                    prompt: finalPrompt,
                    conversation_id: conversationId,
                    assistant_id: parsedAssistantId,
                    assistant_type: assistantData?.assistant_type,
                });
                invoke<AiResponse>("ask_ai", {
                    request: {
                        prompt: finalPrompt,
                        conversation_id: conversationId,
                        assistant_id: parsedAssistantId,
                        attachment_list: fileInfoList?.map((i) => i.id),
                    },
                })
                    .then((res) => {
                        console.log("ask ai response", res);
                        if (conversationId != String(res.conversation_id)) {
                            onChangeConversationId(String(res.conversation_id));
                        }
                    })
                    .catch((error) => {
                        console.error("Send message error:", error);
                        setAiIsResponsing(false);
                        updateShiningMessages();
                    });
            }

            setInputText("");
            clearFileInfoList();
        }
    }, 200);

    return {
        // 对话管理
        handleDeleteConversationSuccess,

        // 消息操作
        handleMessageRegenerate,
        handleMessageEdit,
        handleMessageFork,
        handleEditSave,
        handleEditSaveAndRegenerate,
        handleSend,
        handleArtifact,

        // 编辑对话框状态
        editDialogIsOpen,
        editingMessage,
        closeEditDialog,

        // 标题编辑
        titleEditDialogIsOpen,
        openTitleEditDialog,
        closeTitleEditDialog,
    };
}
