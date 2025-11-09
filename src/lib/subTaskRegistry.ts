/**
 * 全局 Sub-Task 注册表
 *
 * 用于防止多个窗口重复注册相同的 sub-task。
 * 使用 code 作为唯一标识符进行去重。
 */

import { invoke } from "@tauri-apps/api/core";

// 全局注册缓存 - 存储在 window 对象上，跨组件共享
declare global {
    interface Window {
        __subTaskRegistry?: {
            registered: Set<string>;
            pending: Map<string, Promise<void>>;
        };
    }
}

// 初始化全局注册表
function getRegistry() {
    if (!window.__subTaskRegistry) {
        window.__subTaskRegistry = {
            registered: new Set<string>(),
            pending: new Map<string, Promise<void>>(),
        };
    }
    return window.__subTaskRegistry;
}

/**
 * 注册 sub-task（带去重）
 *
 * @param code - Sub-task 唯一标识符
 * @param name - Sub-task 名称
 * @param description - 描述
 * @param systemPrompt - 系统提示词
 * @param pluginSource - 插件来源
 * @param sourceId - 来源 ID
 * @returns Promise<void>
 */
export async function registerSubTask(
    code: string,
    name: string,
    description: string,
    systemPrompt: string,
    pluginSource: string,
    sourceId: number
): Promise<void> {
    const registry = getRegistry();

    // 如果已经注册过，直接返回
    if (registry.registered.has(code)) {
        console.debug(`[SubTaskRegistry] Sub-task '${code}' already registered, skipping...`);
        return;
    }

    // 如果正在注册中，返回现有的 Promise
    const pendingRegistration = registry.pending.get(code);
    if (pendingRegistration) {
        console.debug(`[SubTaskRegistry] Sub-task '${code}' registration in progress, waiting...`);
        return pendingRegistration;
    }

    // 创建新的注册 Promise
    const registrationPromise = (async () => {
        try {
            await invoke("sub_task_regist", {
                code,
                name,
                description,
                systemPrompt,
                pluginSource,
                sourceId,
            });

            // 标记为已注册
            registry.registered.add(code);
            console.debug(`[SubTaskRegistry] Sub-task '${code}' registered successfully`);
        } catch (error) {
            console.error(`[SubTaskRegistry] Failed to register sub-task '${code}':`, error);
            throw error;
        } finally {
            // 清理 pending 状态
            registry.pending.delete(code);
        }
    })();

    // 存储 pending Promise
    registry.pending.set(code, registrationPromise);

    return registrationPromise;
}

/**
 * 检查 sub-task 是否已注册
 */
export function isSubTaskRegistered(code: string): boolean {
    const registry = getRegistry();
    return registry.registered.has(code);
}

/**
 * 清除注册缓存（用于测试或重置）
 */
export function clearSubTaskRegistry(): void {
    const registry = getRegistry();
    registry.registered.clear();
    registry.pending.clear();
    console.debug("[SubTaskRegistry] Registry cleared");
}
