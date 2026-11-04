import { afterEach, expect, it, vi } from "vitest";
import { canLeaveConfigNow, registerConfigBeforeLeave, requestConfigLeave } from "./configLeaveGuard";

afterEach(() => {
    // ensure no leftover handler
    registerConfigBeforeLeave(() => {})();
});

it("runs proceed immediately when no guard is registered", () => {
    const proceed = vi.fn();
    requestConfigLeave(proceed);
    expect(proceed).toHaveBeenCalledOnce();
});

it("defers proceed to the registered dirty-form guard", () => {
    const proceed = vi.fn();
    const unregister = registerConfigBeforeLeave((next) => {
        expect(proceed).not.toHaveBeenCalled();
        next();
    });
    requestConfigLeave(proceed);
    expect(proceed).toHaveBeenCalledOnce();
    unregister();
});

it("reports immediate leave and defers to the prompt otherwise", () => {
    const deferred = vi.fn();
    expect(canLeaveConfigNow(deferred)).toBe(true);
    expect(deferred).not.toHaveBeenCalled();
    let confirm = undefined as (() => void) | undefined;
    const unregister = registerConfigBeforeLeave((next) => { confirm = next; });
    expect(canLeaveConfigNow(deferred)).toBe(false);
    expect(deferred).not.toHaveBeenCalled();
    confirm?.();
    expect(deferred).toHaveBeenCalledOnce();
    unregister();
});
