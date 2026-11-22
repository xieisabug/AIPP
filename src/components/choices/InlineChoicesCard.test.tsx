import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { it, expect, vi, beforeEach } from "vitest";
import InlineChoicesCard from "./InlineChoicesCard";
import type { ChoiceCard, ChoiceDecision } from "@/utils/presentChoices";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("./ChoicePreview", () => ({
    ChoicePreview: ({ candidate }: { candidate: { content: string } }) => <p>{candidate.content}</p>,
    previewReference: vi.fn(),
}));
vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => false }));
vi.mock("@/hooks/use-mobile", () => ({ useIsMobile: () => false }));
beforeEach(() => invoke.mockReset());
it("should submit only the chosen stable ID and feedback when two versions have the same title", async () => {
    const record: ChoiceCard = {
        request_id: "request-A",
        recovered: false,
        request: {
            title: "选开篇",
            selection_mode: "single",
            layout: "tabs",
            allow_feedback: true,
            candidates: [
                { type: "text", id: "first", title: "开篇", content: "第一稿" },
                { type: "text", id: "second", title: "开篇", content: "第二稿" },
            ],
        },
    };
    invoke.mockImplementation(async (name: string, args: { decision: ChoiceDecision }) => {
        if (name === "get_present_choices") return structuredClone(record);
        if (name === "submit_present_choices_response") record.decision = args.decision;
    });
    render(<InlineChoicesCard conversationId={7} callId={12} />);
    await screen.findByText("第一稿");
    fireEvent.click(screen.getAllByRole("button", { name: "开篇" })[1]);
    fireEvent.click(screen.getByRole("radio", { name: "选择 开篇" }));
    fireEvent.change(screen.getByRole("textbox", { name: "补充意见" }), { target: { value: "保留开头" } });
    fireEvent.click(screen.getByRole("button", { name: "确认选择" }));
    await waitFor(() =>
        expect(invoke).toHaveBeenCalledWith("submit_present_choices_response", {
            conversationId: 7,
            requestId: "request-A",
            decision: { status: "selected", selected_ids: ["second"], feedback: "保留开头" },
        })
    );
    expect(await screen.findByText("已选择：开篇")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /展开全部候选/ }));
    expect(screen.getByText("第二稿")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "确认选择" })).toBeNull();
});
