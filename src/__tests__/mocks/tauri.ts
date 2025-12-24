/**
 * Mock Tauri Core API
 *
 * 用于在测试中模拟 @tauri-apps/api/core 的 invoke 调用
 */
import { vi } from "vitest";

// 存储 mock 处理函数的 Map
type InvokeHandler = (args?: Record<string, unknown>) => unknown;
const mockHandlers = new Map<string, InvokeHandler>();

/**
 * Mock invoke 函数
 * 根据命令名调用对应的 mock handler
 */
export const invoke = vi.fn(
    async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
        const handler = mockHandlers.get(cmd);
        if (handler) {
            return handler(args) as T;
        }

        // 默认返回空对象或根据命令返回合理的默认值
        console.warn(`[Mock Tauri] No handler for command: ${cmd}`);
        return {} as T;
    }
);

/**
 * 注册一个 mock handler
 * @param cmd 命令名
 * @param handler 处理函数
 */
export function mockInvokeHandler(cmd: string, handler: InvokeHandler): void {
    mockHandlers.set(cmd, handler);
}

/**
 * 清除指定命令的 mock handler
 * @param cmd 命令名
 */
export function clearMockHandler(cmd: string): void {
    mockHandlers.delete(cmd);
}

/**
 * 清除所有 mock handlers
 */
export function clearAllMockHandlers(): void {
    mockHandlers.clear();
}

/**
 * 获取所有已注册的命令
 */
export function getMockedCommands(): string[] {
    return Array.from(mockHandlers.keys());
}

// 导出其他可能需要的 Tauri API mock
export const convertFileSrc = vi.fn((path: string) => `asset://${path}`);
export const transformCallback = vi.fn();

// 默认导出
export default {
    invoke,
    convertFileSrc,
    transformCallback,
};
