import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { emitTo } from "@tauri-apps/api/event";
import { consumePendingScheduleFocus, openScheduleTask, SCHEDULE_FOCUS_TASK_EVENT } from "./scheduleTaskFocus";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve()) }));
vi.mock("@tauri-apps/api/event", () => ({ emitTo: vi.fn(() => Promise.resolve()) }));

describe("scheduleTaskFocus", () => {
    beforeEach(() => {
        const store = new Map<string, string>();
        vi.stubGlobal("localStorage", {
            getItem: (key: string) => store.get(key) ?? null,
            setItem: (key: string, value: string) => void store.set(key, value),
            removeItem: (key: string) => void store.delete(key),
        });
        vi.clearAllMocks();
    });

    afterEach(() => {
        vi.unstubAllGlobals();
    });

    it("opens the schedule window, emits focus event and leaves a pending id for a fresh window", async () => {
        await openScheduleTask(7);

        expect(invoke).toHaveBeenCalledWith("open_schedule_window");
        expect(emitTo).toHaveBeenCalledWith("schedule", SCHEDULE_FOCUS_TASK_EVENT, { taskId: 7 });
        expect(consumePendingScheduleFocus()).toBe(7);
    });

    it("consumes the pending id only once", async () => {
        await openScheduleTask(3);

        expect(consumePendingScheduleFocus()).toBe(3);
        expect(consumePendingScheduleFocus()).toBeNull();
    });

    it("ignores invalid stored values", () => {
        localStorage.setItem("aipp-schedule-focus-task", "abc");
        expect(consumePendingScheduleFocus()).toBeNull();
        expect(localStorage.getItem("aipp-schedule-focus-task")).toBeNull();
    });
});
