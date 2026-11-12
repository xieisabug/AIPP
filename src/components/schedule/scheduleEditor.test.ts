import { describe, expect, it } from "vitest";
import { buildSchedule, defaultScheduleDraft, draftFromSchedule, type ScheduleDraft } from "./scheduleEditor";

const now = new Date(2026, 10, 12, 10, 0);
const draft = (patch: Partial<ScheduleDraft>): ScheduleDraft => ({ ...defaultScheduleDraft(now), ...patch });

describe("scheduleEditor", () => {
    it("should build schedule strings when each common mode is complete", () => {
        expect(buildSchedule(draft({ mode: "once", date: "2026-11-13", time: "16:00" }))).toBe("2026-11-13 16:00");
        expect(buildSchedule(draft({ mode: "daily", time: "09:05" }))).toBe("5 9 * * *");
        expect(buildSchedule(draft({ mode: "weekly", time: "21:00", weekDays: [6, 2, 2] }))).toBe("0 21 * * 2,6");
        expect(buildSchedule(draft({ mode: "monthly", time: "09:00", monthDays: [15, 1] }))).toBe("0 9 1,15 * *");
        expect(buildSchedule(draft({ mode: "interval", intervalValue: "30", intervalUnit: "m" }))).toBe("every 30m");
        expect(buildSchedule(draft({ mode: "advanced", advanced: " every 1w at 16:00 " }))).toBe("every 1w at 16:00");
    });

    it("should return null when required parts are missing", () => {
        expect(buildSchedule(draft({ mode: "weekly", weekDays: [] }))).toBeNull();
        expect(buildSchedule(draft({ mode: "daily", time: "25:00" }))).toBeNull();
        expect(buildSchedule(draft({ mode: "interval", intervalValue: "0" }))).toBeNull();
        expect(buildSchedule(draft({ mode: "advanced", advanced: "  " }))).toBeNull();
    });

    it("should restore the form mode when editing a saved schedule", () => {
        expect(draftFromSchedule("0 16 * * 4", now)).toMatchObject({ mode: "weekly", time: "16:00", weekDays: [4] });
        expect(draftFromSchedule("5 9 * * *", now)).toMatchObject({ mode: "daily", time: "09:05" });
        expect(draftFromSchedule("0 9 1,15 * *", now)).toMatchObject({ mode: "monthly", monthDays: [1, 15] });
        expect(draftFromSchedule("every 2h", now)).toMatchObject({ mode: "interval", intervalValue: "2", intervalUnit: "h" });
        expect(draftFromSchedule("2026-11-13T16:00:00+08:00", now).mode).toBe("once");
    });

    it("should fall back to advanced mode when the schedule is not a common shape", () => {
        for (const text of ["*/15 * * * *", "0 9 * * 1-5", "every 1w at 16:00", "0 9 1 * 1", "0 9 * 2 *"]) {
            expect(draftFromSchedule(text, now)).toMatchObject({ mode: "advanced", advanced: text });
        }
    });

    it("should round trip common modes", () => {
        for (const text of ["0 16 * * 4", "30 7 * * *", "0 21 * * 2,6", "0 9 1,15 * *", "every 45m"]) {
            expect(buildSchedule(draftFromSchedule(text, now))).toBe(text);
        }
    });
});
