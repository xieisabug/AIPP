import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { it, expect, vi, beforeEach } from "vitest";
import InlineChoicesCard from "./InlineChoicesCard";
import type { ChoiceCard, ChoiceDecision, ChoiceRequest } from "@/utils/presentChoices";
const { invoke, writeText } = vi.hoisted(() => ({ invoke: vi.fn(), writeText: vi.fn(async () => {}) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke, convertFileSrc: (id: string) => "about:blank#" + id }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => false }));
vi.mock("@/hooks/use-mobile", () => ({ useIsMobile: () => false }));
beforeEach(() => {
    invoke.mockReset();
    writeText.mockClear();
});

it.each([
    { scenario: "all three candidates keep their content", changeLast: false },
    { scenario: "one candidate changes while its preparation is pending", changeLast: true },
])("should reuse unchanged grid previews when $scenario", async ({ changeLast }) => {
    const request: ChoiceRequest = {
        title: "选择界面", selection_mode: "single", layout: "grid", allow_feedback: false,
        candidates: ["A", "B", "C"].map(id => ({ id, title: id, type: "html", content: `<p>${id}</p>` })),
    };
    let register!: (card: ChoiceCard) => void;
    const started: string[] = [];
    const pending = new Map<string, () => void>();
    const documents: string[] = [];
    invoke.mockImplementation((name, args) => {
        if (name === "get_present_choices") return new Promise(resolve => { register = resolve; });
        if (name === "prepare_preview_code_request_for_ui") {
            const { code } = args.request;
            started.push(code);
            return new Promise(resolve => pending.set(code, () => resolve(args.request)));
        }
        if (name === "create_choice_document") {
            documents.push(args.html);
            return Promise.resolve(String(documents.length));
        }
        return Promise.resolve();
    });

    const props = { conversationId: 7, llmCallId: "call-grid", parameters: JSON.stringify(request) };
    const card = render(<InlineChoicesCard {...props} isStreaming />);
    const original = request.candidates.map(c => c.content!);
    expect(started).toEqual(original);
    // A、C 尚未返回时，B 可以先显示。
    await act(async () => pending.get(original[1])!());
    const frame = screen.getByTitle("B");
    fireEvent.load(frame);
    expect(documents).toEqual([original[1]]);

    card.rerender(<InlineChoicesCard {...props} callId={12} />);
    await waitFor(() => expect(register).toBeTypeOf("function"));
    const snapshot = structuredClone(request);
    snapshot.candidates[1].title = "方案 B";
    if (changeLast) snapshot.candidates[2].content = "<p>C revised</p>";
    await act(async () => register({ request_id: "req-grid", recovered: false, request: snapshot }));
    expect(screen.getByRole("radio", { name: "选择 方案 B" })).toBeInTheDocument();
    expect(screen.getByTitle("方案 B")).toBe(frame);
    expect(frame.className).not.toContain("opacity-0");
    const finalCode = snapshot.candidates[2].content!;
    expect(started).toEqual(changeLast ? [...original, finalCode] : original);

    await act(async () => { for (const finish of pending.values()) finish(); });
    expect(documents.sort()).toEqual([original[0], original[1], finalCode].sort());
    expect(screen.getByTitle("A")).toBeInTheDocument();
    expect(screen.getByTitle("C")).toBeInTheDocument();
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
    expect(screen.getByRole("combobox", { name: "版本" })).toHaveTextContent("乙");
    expect(screen.getByRole("combobox", { name: "对照版本" })).toHaveTextContent("甲");

    const diff = screen.getByRole("button", { name: "对比差异" });
    fireEvent.click(diff);
    expect(diff).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryAllByRole("article")).toHaveLength(0);
});
