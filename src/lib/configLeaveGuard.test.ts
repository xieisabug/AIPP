import { afterEach, expect, it, vi } from "vitest";
import { registerConfigBeforeLeave, requestConfigLeave } from "./configLeaveGuard";

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
