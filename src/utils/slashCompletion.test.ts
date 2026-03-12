import { describe, expect, it } from "vitest";
import {
    buildSkillInvocation,
    escapeSlashArgument,
    getSlashCompletionContext,
} from "./slashCompletion";

describe("slashCompletion helpers", () => {
    it("detects namespace completion after slash", () => {
        const value = "请执行 /sk";
        const context = getSlashCompletionContext(value, value.length);

        expect(context).toEqual({
            kind: "namespace",
            triggerStart: 4,
            replaceStart: 4,
            replaceEnd: 7,
            query: "sk",
        });
    });

    it("detects skill completion inside skills invocation", () => {
        const value = "/skills(React Best";
        const context = getSlashCompletionContext(value, value.length);

        expect(context).toEqual({
            kind: "skill",
            namespace: "skills",
            triggerStart: 0,
            replaceStart: 0,
            replaceEnd: value.length,
            query: "React Best",
        });
    });

    it("escapes and builds skill invocations", () => {
        expect(escapeSlashArgument(String.raw`Skill (beta) \ helper`)).toBe(
            String.raw`Skill \(beta\) \\ helper`,
        );
        expect(buildSkillInvocation("React Best Practices")).toBe(
            "/skills(React Best Practices)",
        );
    });
});
