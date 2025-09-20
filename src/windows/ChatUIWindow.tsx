import React, { useEffect, useState, useRef } from "react";
import ReactDOM from "react-dom";
import { emit } from "@tauri-apps/api/event";
import ChatUIToolbar from "../components/ChatUIToolbar";
import ConversationList from "../components/ConversationList";
import ChatUIInfomation from "../components/ChatUIInfomation";
import ConversationUI, { ConversationUIRef } from "../components/ConversationUI";
import { useTheme } from "../hooks/useTheme";

import { appDataDir } from "@tauri-apps/api/path";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

function ChatUIWindow() {
    // 集成主题系统
    useTheme();

    const [pluginList, setPluginList] = useState<any[]>([]);

    const [selectedConversation, setSelectedConversation] = useState<string>("");
    const conversationUIRef = useRef<ConversationUIRef>(null);

    // 组件挂载完成后，发送窗口加载事件，通知 AskWindow
    useEffect(() => {
        emit("chat-ui-window-load");

        // 监听 AskWindow 发来的选中对话事件 (只注册一次)
        const unlisten = getCurrentWebviewWindow().listen("select_conversation", (event) => {
            const convId = event.payload;
            if (convId && convId !== "") {
                setSelectedConversation(convId as string);
            }
        });

        // 监听窗口焦点变化
        const windowFocusUnlisten = getCurrentWebviewWindow().onFocusChanged(({ payload: focused }) => {
            if (focused) {
                // 窗口获得焦点时，使用 requestAnimationFrame 聚焦到输入框
                requestAnimationFrame(() => {
                    conversationUIRef.current?.focus();
                });
            }
        });

        return () => {
            unlisten.then((unlisten) => unlisten());
            windowFocusUnlisten.then((unlistenFn) => unlistenFn());
        };
    }, []);

    useEffect(() => {
        // 为可能使用 UMD 构建的插件提供全局 React/ReactDOM（与 PluginWindow 保持一致）
        (window as any).React = React;
        (window as any).ReactDOM = ReactDOM;

        const toPascalCase = (str: string) =>
            str
                .replace(/(^|[-_\s]+)([a-zA-Z0-9])/g, (_, __, c) => (c ? String(c).toUpperCase() : ""))
                .replace(/[^a-zA-Z0-9]/g, "");

        const pluginLoadList = [
            {
                name: "代码生成",
                code: "code-generate",
                pluginType: ["assistantType"],
                instance: null,
            },
            {
                name: "DeepResearch",
                code: "deepresearch",
                pluginType: ["assistantType"],
                instance: null,
            },
        ];

        const initPlugin = async () => {
            const dirPath = await appDataDir();
            const loadPromises = pluginLoadList.map(async (plugin) => {
                const convertFilePath = dirPath + "/plugin/" + plugin.code + "/dist/main.js";

                return new Promise<void>((resolve) => {
                    const script = document.createElement("script");
                    script.src = convertFileSrc(convertFilePath);
                    script.onload = () => {
                        const g: any = window as any;
                        const candidates = [g.SamplePlugin, g[plugin.code], g[toPascalCase(plugin.code)]];
                        const PluginCtor = candidates.find((c) => typeof c === "function");
                        if (PluginCtor) {
                            try {
                                plugin.instance = new PluginCtor();
                                console.debug(`[PluginLoader] '${plugin.code}' instance created via global constructor`);
                            } catch (e) {
                                console.error(`[PluginLoader] Failed to instantiate '${plugin.code}' SamplePlugin:`, e);
                            }
                        } else {
                            console.warn(
                                `[PluginLoader] No global plugin constructor found for '${plugin.code}'. Checked: SamplePlugin, ${plugin.code}, ${toPascalCase(
                                    plugin.code
                                )}. Check plugin UMD global name.`
                            );
                        }
                        resolve();
                    };
                    script.onerror = (error) => {
                        console.error("Failed to load plugin script", plugin.name, error);
                        resolve();
                    };
                    document.body.appendChild(script);
                });
            });

            // 等待所有插件加载完成
            await Promise.all(loadPromises);

            // 所有插件实例都准备好后再更新状态
            setPluginList([...pluginLoadList]);
        };

        initPlugin();
    }, []);

    return (
        <div className="flex h-screen bg-background">
            <div className="flex-none w-[280px] flex flex-col shadow-lg box-border rounded-r-xl mb-2 mr-2">
                <ChatUIInfomation />
                <ChatUIToolbar onNewConversation={() => setSelectedConversation("")} />
                <ConversationList
                    conversationId={selectedConversation}
                    onSelectConversation={setSelectedConversation}
                />
            </div>

            <div className="flex-1 bg-background overflow-auto rounded-xl m-2 ml-0 shadow-lg">
                <ConversationUI
                    ref={conversationUIRef}
                    pluginList={pluginList}
                    conversationId={selectedConversation}
                    onChangeConversationId={setSelectedConversation}
                />
            </div>
        </div>
    );
}

export default ChatUIWindow;
