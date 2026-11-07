import { afterEach, describe, expect, it, vi } from "vitest";
import { navigateMobile, navigateMobileConfig, popMobile, subscribeMobileConfigNavigation } from "./navigation";

describe("mobile settings destinations", () => {
    const subscriptions: Array<() => void> = [];
    afterEach(() => {
        subscriptions.splice(0).forEach(unsubscribe => unsubscribe());
        navigateMobile("chat_ui");
    });

    it("should deliver the target when a lazy settings page subscribes later", () => {
        navigateMobileConfig("assistant-config", "7");
        const receive = vi.fn();
        subscriptions.push(subscribeMobileConfigNavigation(receive));
        expect(receive).toHaveBeenCalledExactlyOnceWith({ menu: "assistant-config", subNav: "7" });
    });

    it("should deliver the latest target when several requests precede mounting", () => {
        navigateMobileConfig("llm-provider-config");
        navigateMobileConfig("assistant-config");
        const receive = vi.fn();
        subscriptions.push(subscribeMobileConfigNavigation(receive));
        expect(receive).toHaveBeenCalledExactlyOnceWith({ menu: "assistant-config", subNav: undefined });
    });

    it("should deliver immediately when settings is already mounted without replaying on remount", () => {
        const receive = vi.fn();
        const unsubscribe = subscribeMobileConfigNavigation(receive);
        navigateMobileConfig("assistant-config");
        unsubscribe();
        subscriptions.push(subscribeMobileConfigNavigation(receive));
        expect(receive).toHaveBeenCalledTimes(1);
    });

    it("should discard the target when settings is dismissed before mounting", () => {
        navigateMobileConfig("assistant-config");
        popMobile();
        const receive = vi.fn();
        subscriptions.push(subscribeMobileConfigNavigation(receive));
        expect(receive).not.toHaveBeenCalled();
    });

    it("should discard a stale target when navigation moves elsewhere before mounting", () => {
        navigateMobileConfig("assistant-config");
        navigateMobile("collections");
        const receive = vi.fn();
        subscriptions.push(subscribeMobileConfigNavigation(receive));
        expect(receive).not.toHaveBeenCalled();
    });
});
