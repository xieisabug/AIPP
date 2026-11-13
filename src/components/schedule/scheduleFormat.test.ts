import { describe, expect, it } from "vitest";
import { formatRunDuration, formatShortDateTime, parseToolLogPayload, runStatusLabel, taskGroup } from "./scheduleFormat";

describe("scheduleFormat", () => {
    it("labels every run status including the new outcomes", () => {
        expect(runStatusLabel("running")).toBe("运行中");
        expect(runStatusLabel("delivered")).toBe("已发送");
        expect(runStatusLabel("quiet")).toBe("无新内容");
        expect(runStatusLabel("skipped")).toBe("跳过");
        expect(runStatusLabel("cancelled")).toBe("已停止");
        expect(runStatusLabel("failed")).toBe("失败");
        expect(runStatusLabel("missed")).toBe("错过");
        expect(runStatusLabel("unknown")).toBe("unknown");
    });

    it("groups tasks by what the user needs to do", () => {
        expect(taskGroup({ enabled: true, needsAttention: true, pausedReason: null })).toBe("attention");
        expect(taskGroup({ enabled: true, needsAttention: false, pausedReason: null })).toBe("active");
        expect(taskGroup({ enabled: false, needsAttention: false, pausedReason: "已手动暂停" })).toBe("paused");
        expect(taskGroup({ enabled: false, needsAttention: false, pausedReason: "已完成" })).toBe("finished");
    });

    it("formats run duration in readable units", () => {
        const start = "2026-11-07T00:00:00Z";
        expect(formatRunDuration(start, "2026-11-07T00:00:48Z")).toBe("48 秒");
        expect(formatRunDuration(start, "2026-11-07T00:03:12Z")).toBe("3 分 12 秒");
        expect(formatRunDuration(start, "2026-11-07T00:05:00Z")).toBe("5 分");
        expect(formatRunDuration(start, "2026-11-07T01:05:30Z")).toBe("1 小时 5 分");
        expect(formatRunDuration(start, null)).toBeNull();
        expect(formatRunDuration(start, "not-a-date")).toBeNull();
    });

    it("omits the year only when it matches the current year", () => {
        const now = new Date(2026, 10, 7, 12, 0);
        expect(formatShortDateTime(new Date(2026, 10, 8, 8, 0).toISOString(), now)).toBe("11-08 08:00");
        expect(formatShortDateTime(new Date(2025, 0, 2, 9, 5).toISOString(), now)).toBe("2025-01-02 09:05");
        expect(formatShortDateTime(null, now)).toBe("");
    });

    it("parses tool payloads only for tool log types", () => {
        const base = { id: 1, taskId: 1, runId: "r", createdTime: "" };
        expect(parseToolLogPayload({ ...base, messageType: "tool_call", content: '{"toolName":"fetch_url"}' }))
            .toEqual({ toolName: "fetch_url" });
        expect(parseToolLogPayload({ ...base, messageType: "tool_result", content: "oops" })).toBeNull();
        expect(parseToolLogPayload({ ...base, messageType: "start", content: '{"toolName":"x"}' })).toBeNull();
    });
});
