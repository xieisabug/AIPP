import { invoke } from "@tauri-apps/api/core";
import { emitTo } from "@tauri-apps/api/event";

/** 定时任务窗口收到后切换到指定任务。 */
export const SCHEDULE_FOCUS_TASK_EVENT = "schedule-window-focus-task";
/**
 * 同一请求也写入 localStorage（各窗口同源共享）：定时任务窗口若是刚创建、
 * 事件监听尚未注册，挂载时仍能读到待定位的任务。
 */
const SCHEDULE_FOCUS_TASK_STORAGE_KEY = "aipp-schedule-focus-task";

export interface ScheduleFocusTaskPayload {
    taskId: number;
}

/** 打开定时任务窗口并定位到指定任务。 */
export async function openScheduleTask(taskId: number): Promise<void> {
    try {
        localStorage.setItem(SCHEDULE_FOCUS_TASK_STORAGE_KEY, String(taskId));
    } catch {
        // localStorage 不可用时只依赖事件
    }
    await invoke("open_schedule_window");
    await emitTo<ScheduleFocusTaskPayload>("schedule", SCHEDULE_FOCUS_TASK_EVENT, { taskId });
}

/** 读取并清除待定位的任务 id（定时任务窗口挂载或收到事件时调用）。 */
export function consumePendingScheduleFocus(): number | null {
    try {
        const raw = localStorage.getItem(SCHEDULE_FOCUS_TASK_STORAGE_KEY);
        if (raw === null) return null;
        localStorage.removeItem(SCHEDULE_FOCUS_TASK_STORAGE_KEY);
        const id = Number(raw);
        return Number.isInteger(id) && id > 0 ? id : null;
    } catch {
        return null;
    }
}
