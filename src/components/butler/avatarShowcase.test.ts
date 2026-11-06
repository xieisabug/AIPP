import { describe, expect, it } from "vitest";
import { accessoryActionsOf, showcaseDelay, showcaseStep } from "./avatarShowcase";

describe("avatar editor showcase", () => {
    it("should alternate states with actions and feature equipped decorations", () => {
        const actions = accessoryActionsOf(["sunglasses", "bow-bell"]);
        expect(actions).toEqual(["peer", "jingle"]);
        expect(showcaseStep(0, actions)).toEqual({ kind: "state", state: "idle" });
        const played = [1, 3, 5, 7, 9, 11].map(step => showcaseStep(step, actions));
        expect(played.every(step => step.kind === "action")).toBe(true);
        expect(played.map(step => step.kind === "action" && step.action)).toEqual(["peer", "jingle", "smile", "peer", "jingle", "blink"]);
    });

    it("should hold each scene for three to five seconds and never cut an action short", () => {
        for (let seed = 0; seed < 50; seed++) {
            const delay = showcaseDelay({ kind: "state", state: "idle" }, seed);
            expect(delay).toBeGreaterThanOrEqual(3000);
            expect(delay).toBeLessThanOrEqual(5000);
            expect(showcaseDelay({ kind: "action", action: "peer" }, seed)).toBeGreaterThanOrEqual(3600);
        }
    });
});
