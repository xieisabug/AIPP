import { afterEach, describe, expect, it } from "vitest";
import {
    claimAgentConnectionEvent,
    resetAgentConnectionEventsForTest,
} from "./agentSessionNotice";

describe("agent session restore notice", () => {
    afterEach(() => {
        resetAgentConnectionEventsForTest();
    });

    it("should show once when the same connection event is observed repeatedly", () => {
        expect(claimAgentConnectionEvent("run-1:resume")).toBe(true);
        expect(claimAgentConnectionEvent("run-1:resume")).toBe(false);
    });

    it("should show again when a new connection generation resumes the session", () => {
        expect(claimAgentConnectionEvent("run-1:resume")).toBe(true);
        expect(claimAgentConnectionEvent("run-2:resume")).toBe(true);
    });
});
