import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import MobileTasksConfig from "./MobileTasksConfig";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@/hooks/useMobileResume", () => ({ useMobileResume: vi.fn() }));

describe("mobile task controls", () => {
    beforeEach(() => {
        vi.clearAllMocks();
        vi.mocked(invoke).mockImplementation(async command => {
            if (command === "get_mobile_butler_overview") return { main_conversation_id: 1, tasks: [{ task_conversation_id: 2, title: "同步任务", status: "running", is_running: false }], local_permissions: [] };
            if (command === "list_scheduled_tasks") return [{ id: 3, name: "计划任务", isEnabled: true, taskPrompt: "检查状态" }];
            if (command === "get_butler_feishu_runtime_status") return { configured: false, running: false, connected: false, status_text: "未配置" };
            if (command === "get_assistants") return [{ id: 4, name: "API 助手", assistant_type: 0 }];
            if (command === "list_scheduled_task_runs") return { runs: [{ runId: "remote-1", status: "running", localRunning: false }, { runId: "missed-1", status: "missed", localRunning: false }] };
            throw new Error(`unexpected command: ${command}`);
        });
    });
    it("should omit progress cards and their control queries when tasks are loaded", async () => {
        render(<MobileTasksConfig />);
        await screen.findByText("同步任务");
        expect(screen.queryByRole("region", { name: /工作进展/ })).toBeNull();
        expect(screen.queryByText("下一步：", { exact: false })).toBeNull();
        expect(screen.getByText("查看详情")).toBeTruthy();
        expect(invoke).not.toHaveBeenCalledWith("get_butler_task_control", expect.anything());
    });
    it("should show remote running records as unknown without control when no local runtime exists", async () => {
        render(<MobileTasksConfig />);
        await screen.findByText("同步任务");
        expect(screen.queryByText("取消本机执行")).toBeNull();
        fireEvent.click(screen.getByText("查看运行记录"));
        await screen.findByText("结果未知（本机没有运行实例）");
        expect(screen.getByText("已错过，未执行")).toBeTruthy();
        expect(screen.queryByText("停止本机运行")).toBeNull();
        expect(invoke).not.toHaveBeenCalledWith("load_butler_main_conversation", expect.anything());
        expect(invoke).not.toHaveBeenCalledWith("run_scheduled_task_now", expect.anything());
    });
    it("should preserve task input and reject duplicate clicks when dispatch fails", async () => {
        const original = vi.mocked(invoke).getMockImplementation()!;
        let rejectDispatch: (reason: unknown) => void = () => {};
        vi.mocked(invoke).mockImplementation((command, args) => command === "spawn_butler_task_conversation" ? new Promise((_, reject) => { rejectDispatch = reject; }) : original(command, args));
        render(<MobileTasksConfig />);
        await screen.findByText("API 助手");
        fireEvent.change(screen.getByRole("combobox"), { target: { value: "4" } });
        fireEvent.change(screen.getByLabelText("任务标题"), { target: { value: "测试任务" } });
        fireEvent.change(screen.getByLabelText("任务目标"), { target: { value: "保留内容" } });
        fireEvent.click(screen.getByText("在本机派发"));
        fireEvent.click(screen.getByText("在本机派发"));
        expect(vi.mocked(invoke).mock.calls.filter(([name]) => name === "spawn_butler_task_conversation")).toHaveLength(1);
        rejectDispatch(new Error("MOBILE_UNSUPPORTED: assistant_id=4; CLI"));
        await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("assistant_id=4"));
        expect((screen.getByLabelText("任务目标") as HTMLTextAreaElement).value).toBe("保留内容");
    });
    it("should discard old run records when another task is selected during refresh", async () => {
        const original = vi.mocked(invoke).getMockImplementation()!;
        let readsA = 0;
        let resolveOld: (value: unknown) => void = () => {};
        vi.mocked(invoke).mockImplementation((command, args: any) => {
            if (command === "list_scheduled_tasks") return Promise.resolve([
                { id: 3, name: "任务A", isEnabled: true, taskPrompt: "A" },
                { id: 5, name: "任务B", isEnabled: true, taskPrompt: "B" },
            ]);
            if (command === "list_scheduled_task_runs") {
                if (args.taskId === 3 && ++readsA > 1) return new Promise(resolve => { resolveOld = resolve; });
                return Promise.resolve({ runs: [{ runId: args.taskId === 3 ? "run-A" : "run-B", status: "running", localRunning: true }] });
            }
            return original(command, args);
        });
        render(<MobileTasksConfig />);
        await screen.findByText("任务A");
        fireEvent.click(screen.getAllByText("查看运行记录")[0]);
        await screen.findByText("run-A");
        await waitFor(() => expect(screen.getAllByText("查看运行记录")[1]).not.toBeDisabled());
        fireEvent.click(screen.getByText("刷新本机状态"));
        await waitFor(() => expect(readsA).toBeGreaterThan(1));
        fireEvent.click(screen.getAllByText("查看运行记录")[1]);
        await screen.findByText("run-B");
        await act(async () => { resolveOld({ runs: [{ runId: "stale-A", status: "running", localRunning: true }] }); });
        expect(screen.queryByText("stale-A")).toBeNull();
        expect(screen.getByText("run-B")).toBeTruthy();
    });
    it("should expose stop while a manual run is pending without opening records first", async () => {
        const original = vi.mocked(invoke).getMockImplementation()!;
        let running = false;
        let finish: (value: unknown) => void = () => {};
        vi.mocked(invoke).mockImplementation((command, args) => {
            if (command === "run_scheduled_task_now") {
                running = true;
                return new Promise(resolve => { finish = resolve; });
            }
            if (command === "list_scheduled_task_runs") return Promise.resolve({ runs: [{ runId: "local-run", status: running ? "running" : "cancelled", localRunning: running }] });
            if (command === "stop_scheduled_task_run") {
                running = false;
                finish({ success: false, error: "cancelled by fixture" });
                return Promise.resolve();
            }
            return original(command, args);
        });
        render(<MobileTasksConfig />);
        fireEvent.click(await screen.findByText("在本机运行一次"));
        const stop = await screen.findByText("停止本机运行");
        expect(stop).not.toBeDisabled();
        fireEvent.click(stop);
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("stop_scheduled_task_run", { taskId: 3, runId: "local-run" }));
        await waitFor(() => expect(screen.queryByText("停止本机运行")).toBeNull());
    });
});
