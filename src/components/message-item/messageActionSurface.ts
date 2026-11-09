import { createContext, useContext } from "react";

/**
 * 消息操作当前渲染在哪里：悬浮操作条（只放图标）还是右键菜单（图标 + 文字的菜单项）。
 * 插件操作是任意节点，菜单无法拆出它们的图标和文字；内置操作（如管家「引用回复」）读这个上下文自行切换形态。
 */
export type MessageActionSurface = "toolbar" | "menu";

export const MessageActionSurfaceContext = createContext<MessageActionSurface>("toolbar");

export const useMessageActionSurface = () => useContext(MessageActionSurfaceContext);
