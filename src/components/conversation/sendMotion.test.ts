import { afterEach, describe, expect, it, vi } from "vitest";
import { afterSendMotion, holdForSendMotion, isSendMotionRunning } from "./sendMotion";

const flushMicrotasks = () => Promise.resolve();

describe("sendMotion", () => {
    afterEach(() => {
        vi.useRealTimers();
    });

    it("should run a task immediately when no send motion is running", () => {
        const task = vi.fn();
        afterSendMotion(task);
        expect(task).toHaveBeenCalledOnce();
    });

    it("should defer tasks until the motion ends and keep their order", async () => {
        const order: string[] = [];
        const release = holdForSendMotion();
        afterSendMotion(() => order.push("first"));
        afterSendMotion(() => order.push("second"));
        expect(order).toEqual([]);

        release();
        // Arrives after the release but before the queued tasks ran: it must not overtake them.
        afterSendMotion(() => order.push("third"));
        expect(order).toEqual([]);
        await flushMicrotasks();
        expect(order).toEqual(["first", "second", "third"]);
        expect(isSendMotionRunning()).toBe(false);
    });

    it("should wait for every overlapping motion", async () => {
        const task = vi.fn();
        const first = holdForSendMotion();
        const second = holdForSendMotion();
        afterSendMotion(task);
        first();
        first();
        await flushMicrotasks();
        expect(task).not.toHaveBeenCalled();
        second();
        await flushMicrotasks();
        expect(task).toHaveBeenCalledOnce();
    });

    it("should release a motion that never ends after the cap", async () => {
        vi.useFakeTimers();
        const task = vi.fn();
        const release = holdForSendMotion(300);
        afterSendMotion(task);
        vi.advanceTimersByTime(299);
        await flushMicrotasks();
        expect(task).not.toHaveBeenCalled();
        vi.advanceTimersByTime(1);
        await flushMicrotasks();
        expect(task).toHaveBeenCalledOnce();
        release();
        expect(isSendMotionRunning()).toBe(false);
    });

    it("should keep running later tasks when one throws", async () => {
        const error = vi.spyOn(console, "error").mockImplementation(() => {});
        const task = vi.fn();
        const release = holdForSendMotion();
        afterSendMotion(() => { throw new Error("boom"); });
        afterSendMotion(task);
        release();
        await flushMicrotasks();
        expect(task).toHaveBeenCalledOnce();
        expect(error).toHaveBeenCalled();
        error.mockRestore();
    });
});
