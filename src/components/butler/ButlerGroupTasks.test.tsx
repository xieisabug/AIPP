import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { ButlerGroupTasks } from "./ButlerGroupTasks";
import type { ButlerGroupMember, ButlerGroupMessage, ButlerGroupTask } from "@/data/ButlerGroup";

const api = vi.hoisted(() => ({ startButlerGroupTask: vi.fn(), stopButlerGroupTask: vi.fn() }));
vi.mock("@/services/butlerGroupService", () => api);
const member: ButlerGroupMember = { id: 20, group_id: 1, assistant_id: 2, name: "研究员", conversation_id: 200, active: true, joined_after: 0, delivered_through: 0 };
const messages: ButlerGroupMessage[] = [1, 2].map(seq => ({ group_id: 1, seq, sender_member: 20, sender_name: "研究员", kind: "result", body: `资料 ${seq}`, reply_to: null, reference_only: false, created_at: "现在" }));
const task: ButlerGroupTask = { conversation_id: 301, member_id: 20, title: "核对资料", parent_packet: null, state: "running" };
const refresh = vi.fn();
const props = { groupId: 1, members: [member], messages, tasks: [] as ButlerGroupTask[], stopped: false, refresh };
beforeEach(() => {
    vi.resetAllMocks();
    api.startButlerGroupTask.mockResolvedValue({ conversation_id: 301 });
    api.stopButlerGroupTask.mockResolvedValue([]);
    refresh.mockResolvedValue(undefined);
});
function fill() {
    fireEvent.click(screen.getByRole("button", { name: "新建任务" }));
    fireEvent.change(screen.getByLabelText("任务执行成员"), { target: { value: "20" } });
    fireEvent.change(screen.getByLabelText("独立任务标题"), { target: { value: "核对资料" } });
    fireEvent.change(screen.getByLabelText("独立任务要求"), { target: { value: "提供来源" } });
}

it("should include only explicitly selected public sources when creating an isolated task", async () => {
    render(<ButlerGroupTasks {...props} />); fill();
    fireEvent.click(screen.getByRole("checkbox", { name: /#2/ }));
    fireEvent.click(screen.getByRole("button", { name: "创建独立任务" }));
    await waitFor(() => expect(api.startButlerGroupTask).toHaveBeenCalledWith(1, expect.objectContaining({ member_id: 20, title: "核对资料", prompt: "提供来源", source_seqs: [2] })));
    expect(api.startButlerGroupTask.mock.calls[0][1].request_key).toEqual(expect.any(String));
});

it("should reuse the request identity when retrying after an uncertain creation response", async () => {
    api.startButlerGroupTask.mockRejectedValueOnce(new Error("响应丢失"));
    render(<ButlerGroupTasks {...props} />); fill();
    fireEvent.click(screen.getByRole("button", { name: "创建独立任务" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "创建独立任务" })).not.toBeDisabled());
    fireEvent.click(screen.getByRole("button", { name: "创建独立任务" }));
    await waitFor(() => expect(api.startButlerGroupTask).toHaveBeenCalledTimes(2));
    expect(api.startButlerGroupTask.mock.calls[1]).toEqual(api.startButlerGroupTask.mock.calls[0]);
});

it("should block submission when the group stops while the creation dialog is open", () => {
    const view = render(<ButlerGroupTasks {...props} />); fill();
    view.rerender(<ButlerGroupTasks {...props} stopped />);
    expect(screen.getByRole("button", { name: "创建独立任务" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "创建独立任务" }));
    expect(api.startButlerGroupTask).not.toHaveBeenCalled();
});

it("should retain individual stop failures when refreshing the task list also fails", async () => {
    api.stopButlerGroupTask.mockResolvedValue([{ succeeded: true, detail: "已停止" }, { succeeded: false, detail: "conversation=302; run=r7; process cancel failed" }]);
    refresh.mockRejectedValue(new Error("列表不可用"));
    render(<ButlerGroupTasks {...props} tasks={[task]} />);
    fireEvent.click(screen.getByRole("button", { name: "停止任务及子任务" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("process cancel failed"));
    expect(screen.getByRole("alert")).toHaveTextContent("列表刷新失败");
    expect(api.stopButlerGroupTask).toHaveBeenCalledWith(1, 301);
});

it("should report confirmed creation separately when only the list refresh fails", async () => {
    refresh.mockRejectedValue(new Error("列表不可用"));
    render(<ButlerGroupTasks {...props} />); fill();
    fireEvent.click(screen.getByRole("button", { name: "创建独立任务" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("任务已创建，但列表刷新失败"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(api.startButlerGroupTask).toHaveBeenCalledTimes(1);
});
