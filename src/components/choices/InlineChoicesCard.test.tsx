import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { it, expect, vi, beforeEach } from "vitest";
import InlineChoicesCard from "./InlineChoicesCard";
import type { ChoiceCard, ChoiceDecision } from "@/utils/presentChoices";
const { invoke, writeText } = vi.hoisted(() => ({ invoke: vi.fn(), writeText: vi.fn(async () => {}) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("./ChoicePreview", () => ({
    ChoicePreview: ({ candidate }: { candidate: { content: string } }) => <p>{candidate.content}</p>,
    previewReference: vi.fn(),
}));
vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => false }));
vi.mock("@/hooks/use-mobile", () => ({ useIsMobile: () => false }));
beforeEach(() => {
    invoke.mockReset();
    writeText.mockClear();
});
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
    fireEvent.click(screen.getAllByRole("tab", { name: "开篇" })[1]);
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
    expect(await screen.findByText("已选择")).toBeInTheDocument();
    expect(screen.getByText("保留开头", { exact: false })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "复制" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("第二稿"));
    fireEvent.click(screen.getByRole("button", { name: /展开全部候选/ }));
    expect(screen.getByText("第二稿")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "确认选择" })).toBeNull();
});

it("should switch layout, swap sides and toggle diff through icon controls when the record is ready", async () => {
    const record: ChoiceCard = {
        request_id: "request-B",
        recovered: false,
        request: {
            title: "选标语",
            selection_mode: "single",
            layout: "tabs",
            allow_feedback: false,
            candidates: [
                { type: "text", id: "a", title: "甲", content: "简洁有力" },
                { type: "text", id: "b", title: "乙", content: "温和亲切" },
            ],
        },
    };
    invoke.mockImplementation(async (name: string) => (name === "get_present_choices" ? structuredClone(record) : undefined));
    render(<InlineChoicesCard conversationId={7} callId={13} />);
    await screen.findByText("简洁有力");
    expect(screen.queryByRole("radiogroup", { name: "预览宽度" })).toBeNull();
    expect(screen.getByRole("button", { name: "全屏对比" })).toBeInTheDocument();

    const split = screen.getByRole("radio", { name: "并排" });
    fireEvent.click(split);
    expect(split).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByRole("tablist")).toBeNull();
    expect(screen.getByText("温和亲切")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "交换两侧版本" }));
    expect(screen.getByRole("combobox", { name: "版本" })).toHaveValue("1");
    expect(screen.getByRole("combobox", { name: "对照版本" })).toHaveValue("0");

    const diff = screen.getByRole("button", { name: "对比差异" });
    fireEvent.click(diff);
    expect(diff).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryAllByRole("article")).toHaveLength(0);
});
