import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AcpPermissionDialog } from "./OperationPermissionDialog";

const request = {
    request_id: "pi:1:run:2:req",
    conversation_id: 1,
    agent_kind: "pi",
    tool_call_id: "req",
    title: "允许 Pi 使用 bash",
    kind: "native_agent_permission",
    parameters: "{\"command\":\"rm -rf build\"}",
    options: [{ option_id: "allow", name: "本次允许", kind: "allow_once" }, { option_id: "deny", name: "拒绝", kind: "reject_once" }],
};

describe("AcpPermissionDialog", () => {
    it("should explain why AIPP auto-review handed the request to the user", () => {
        render(<AcpPermissionDialog request={{ ...request, review: { verdict: "risky", reason: "自动审核认为需要人工确认：递归删除目录" } }} isOpen onDecision={vi.fn()} />);
        expect(screen.getByRole("status")).toHaveTextContent("递归删除目录");
        expect(screen.getByText("Pi 工具权限请求")).toBeInTheDocument();
    });

    it("should not show a review note for requests that were never reviewed", () => {
        render(<AcpPermissionDialog request={request} isOpen onDecision={vi.fn()} />);
        expect(screen.queryByRole("status")).not.toBeInTheDocument();
    });
});
