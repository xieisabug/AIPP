/**
 * Vitest 测试环境全局配置
 *
 * 这个文件在每个测试文件运行前自动执行
 */
import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";

type EventCallback = (event: { payload: any }) => void;
const eventListeners = new Map<string, Set<EventCallback>>();
const onceListeners = new Map<string, Set<EventCallback>>();

// 每个测试后自动清理 React 组件
afterEach(() => {
    cleanup();
});

// Mock Tauri API
vi.mock("@tauri-apps/api/core", () => import("./mocks/tauri"));

// Mock Tauri 事件 API
vi.mock("@tauri-apps/api/event", () => ({
    listen: vi.fn(async (event: string, handler: EventCallback) => {
        const handlers = eventListeners.get(event) ?? new Set<EventCallback>();
        handlers.add(handler);
        eventListeners.set(event, handlers);
        return () => {
            handlers.delete(handler);
        };
    }),
    emit: vi.fn(async (event: string, payload?: any) => {
        const handlers = eventListeners.get(event);
        if (handlers) {
            handlers.forEach((handler) => handler({ payload }));
        }
        const onceHandlers = onceListeners.get(event);
        if (onceHandlers && onceHandlers.size > 0) {
            onceHandlers.forEach((handler) => handler({ payload }));
            onceListeners.delete(event);
        }
    }),
    once: vi.fn(async (event: string, handler: EventCallback) => {
        const handlers = onceListeners.get(event) ?? new Set<EventCallback>();
        handlers.add(handler);
        onceListeners.set(event, handlers);
        return () => {
            handlers.delete(handler);
        };
    }),
}));

afterEach(() => {
    eventListeners.clear();
    onceListeners.clear();
});

// Mock window.__TAURI__
Object.defineProperty(window, "__TAURI__", {
    value: {
        invoke: vi.fn(),
    },
    writable: true,
});
