import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import ScheduleTaskDetail, { type ScheduleTaskDetailProps } from "./ScheduleTaskDetail";
import type { ScheduledTask, ScheduledTaskRun } from "./scheduleTypes";

const task: ScheduledTask = {
    id: 1,
    name: "长沙每日天气早报",
    isEnabled: true,
    scheduleType: "interval",
    intervalValue: 1,
    intervalUnit: "day",
    startTime: "08:00",
    assistantId: 1,
    taskPrompt: "短指令",
    notifyPrompt: "",
    butlerConversationId: 1467,
    createdTime: "2026-11-06T23:55:44Z",
    updatedTime: "2026-11-07T00:00:23Z",
};

const run = (overrides: Partial<ScheduledTaskRun>): ScheduledTaskRun => ({
    id: 1,
    taskId: 1,
    runId: "run-1-abcdef",
    status: "success",
    notify: false,
    startedTime: "2026-11-07T00:00:00Z",
    finishedTime: "2026-11-07T00:00:48Z",
    ...overrides,
});

function renderDetail(overrides: Partial<ScheduleTaskDetailProps> = {}) {
    const props: ScheduleTaskDetailProps = {
        task,
        scheduleDescription: "每 1 天 08:00 执行",
        assistantName: "天气助手",
        runs: [],
        selectedRunId: null,
        isRunsLoading: false,
        logs: [],
        isLogsLoading: false,
        runningRun: null,
        isRunning: false,
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
    it("marks butler tasks and shows missed runs as 错过 instead of 完成", () => {
        renderDetail({
            runs: [run({ id: 2, runId: "run-missed", status: "missed", finishedTime: null })],
        });
        expect(screen.getByText("管家")).toBeTruthy();
        expect(screen.getByText("错过")).toBeTruthy();
        expect(screen.queryByText("完成")).toBeNull();
    });

    it("shows run duration and selects a run on click", () => {
        const props = renderDetail({ runs: [run({})] });
        expect(screen.getByText(/48 秒/)).toBeTruthy();
        fireEvent.click(screen.getByText("完成"));
        expect(props.onSelectRun).toHaveBeenCalledWith("run-1-abcdef");
    });

    it("swaps 立即执行 for 停止 while a run is in progress", () => {
        const running = run({ status: "running", finishedTime: null });
        const props = renderDetail({ runs: [running], runningRun: running });
        expect(screen.queryByText("立即执行")).toBeNull();
        fireEvent.click(screen.getByText("停止"));
        expect(props.onStop).toHaveBeenCalled();
    });

    it("lets long prompts expand and keeps short prompts without a toggle", () => {
        renderDetail({ task: { ...task, taskPrompt: "很长的指令".repeat(80) } });
        const toggle = screen.getByRole("button", { name: "展开" });
        fireEvent.click(toggle);
        expect(toggle.getAttribute("aria-expanded")).toBe("true");
        expect(screen.getByText("收起")).toBeTruthy();
        // 通知规则为空：显示默认规则文案且没有展开按钮
        expect(screen.getByText("默认规则：有重要信息时通知")).toBeTruthy();
        expect(screen.getAllByRole("button", { name: /展开|收起/ })).toHaveLength(1);
    });

    it("asks to pick a run before showing logs", () => {
        renderDetail();
        expect(screen.getByText("暂无运行记录")).toBeTruthy();
        expect(screen.getByText("选择左侧一条运行记录查看日志")).toBeTruthy();
    });
});
