import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import ScheduleTaskDetail, { type ScheduleTaskDetailProps } from "./ScheduleTaskDetail";
import type { ScheduledRun, ScheduledTask, ScheduledTaskLog } from "./scheduleTypes";

const task: ScheduledTask = {
    id: 6,
    name: "周四休息提醒",
    enabled: true,
    schedule: "0 16 * * 4",
    scheduleText: "每周四 16:00",
    upcoming: ["2026-11-19T08:00:00+00:00"],
    upcomingText: ["2026-11-19 16:00 周四", "2026-11-26 16:00 周四"],
    assistantId: 40,
    assistantName: "健身教练",
    prompt: "提醒用户今天休息",
    deliverPolicy: "always",
    deliverTo: "owner_chat",
    deliverTargetText: "与健身教练的私聊",
    butlerMode: true,
    runCount: 3,
    missedPolicy: "run_once",
    consecutiveFailures: 0,
    createdTime: "2026-11-12T02:00:00Z",
    updatedTime: "2026-11-12T02:00:00Z",
    needsAttention: false,
};

const run = (overrides: Partial<ScheduledRun>): ScheduledRun => ({
    runId: "run-1",
    status: "delivered",
    statusText: "已发送",
    startedTime: "2026-11-12T08:00:00Z",
    finishedTime: "2026-11-12T08:00:48Z",
    scheduledFor: "2026-11-12T08:00:00Z",
    localRunning: false,
    ...overrides,
});

const log = (id: number, messageType: string, content: string): ScheduledTaskLog => ({
    id, taskId: 6, runId: "run-1", messageType, content, createdTime: "2026-11-12T08:00:10Z",
});

function renderDetail(overrides: Partial<ScheduleTaskDetailProps> = {}) {
    const props: ScheduleTaskDetailProps = {
        task,
        runs: [],
        selectedRunId: null,
        isRunsLoading: false,
        logs: [],
        isLogsLoading: false,
        isStarting: false,
        isStopping: false,
        onSelectRun: vi.fn(),
        onRefreshRuns: vi.fn(),
        onToggleEnabled: vi.fn(),
        onEdit: vi.fn(),
        onRunNow: vi.fn(),
        onStop: vi.fn(),
        onDelete: vi.fn(),
        onOpenToolLog: vi.fn(),
        ...overrides,
    };
    render(<ScheduleTaskDetail {...props} />);
    return props;
}

describe("ScheduleTaskDetail", () => {
    it("should show the readable schedule, upcoming times and delivery target when a task is selected", () => {
        renderDetail();
        expect(screen.getByText("每周四 16:00")).toBeTruthy();
        expect(screen.getByText(/接下来：2026-11-19 16:00 周四/)).toBeTruthy();
        expect(screen.getByText("与健身教练的私聊")).toBeTruthy();
        expect(screen.getByText("每次都发")).toBeTruthy();
        expect(screen.getByText(/3 次/)).toBeTruthy();
    });

    it("should explain why a task needs attention when it was auto-paused", () => {
        renderDetail({
            task: { ...task, enabled: false, needsAttention: true, consecutiveFailures: 5, pausedReason: "连续失败 5 次，已自动暂停" },
        });
        expect(screen.getByText("需要处理")).toBeTruthy();
        expect(screen.getByRole("status").textContent).toContain("连续失败 5 次，已自动暂停");
        expect(screen.getByRole("status").textContent).toContain("不会补跑");
    });

    it("should label skipped runs with their reason when the run never started", () => {
        renderDetail({ runs: [run({ runId: "s", status: "skipped", statusText: "跳过", reason: "执行助手 #40 不存在" })] });
        expect(screen.getByText("跳过")).toBeTruthy();
        expect(screen.getByText("执行助手 #40 不存在")).toBeTruthy();
    });

    it("should select a run on click and show its delivered output", () => {
        const props = renderDetail({
            runs: [run({})],
            selectedRunId: "run-1",
            logs: [log(1, "start", "开始执行定时任务"), log(2, "response", "今天是休息日，记得拉伸。"), log(3, "delivered", "已发送到会话 #1529")],
        });
        expect(screen.getByText(/已发送到与健身教练的私聊/)).toBeTruthy();
        expect(screen.getAllByText("今天是休息日，记得拉伸。").length).toBeGreaterThan(0);
        fireEvent.click(screen.getAllByText("已发送")[0]);
        expect(props.onSelectRun).toHaveBeenCalledWith("run-1");
    });

    it("should swap run-now for stop while this machine runs the task", () => {
        const running = run({ status: "running", statusText: "运行中", finishedTime: null, localRunning: true });
        const props = renderDetail({ runs: [running] });
        expect(screen.queryByText("立即运行")).toBeNull();
        fireEvent.click(screen.getByText("停止"));
        expect(props.onStop).toHaveBeenCalledWith(running);
    });

    it("should let long prompts expand", () => {
        renderDetail({ task: { ...task, prompt: "很长的指令".repeat(80) } });
        const toggle = screen.getByRole("button", { name: "展开" });
        fireEvent.click(toggle);
        expect(toggle.getAttribute("aria-expanded")).toBe("true");
    });

    it("should ask to pick a run when none is selected", () => {
        renderDetail();
        expect(screen.getByText("还没有运行过")).toBeTruthy();
        expect(screen.getByText("选择左侧一条运行记录查看过程")).toBeTruthy();
    });
});
