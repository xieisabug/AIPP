/**
 * Vitest 测试环境全局配置
 *
 * 这个文件在每个测试文件运行前自动执行
 */
import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";

// 每个测试后自动清理 React 组件
afterEach(() => {
    cleanup();
});

// Mock Tauri API
vi.mock("@tauri-apps/api/core", () => import("./mocks/tauri"));

// Mock Tauri 事件 API
vi.mock("@tauri-apps/api/event", () => ({
    listen: vi.fn(() => Promise.resolve(() => {})),
    emit: vi.fn(() => Promise.resolve()),
    once: vi.fn(() => Promise.resolve(() => {})),
}));

// Mock window.__TAURI__
Object.defineProperty(window, "__TAURI__", {
    value: {
        invoke: vi.fn(),
    },
    writable: true,
});
