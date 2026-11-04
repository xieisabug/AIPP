import { useState } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { emit } from "@tauri-apps/api/event";
import InputArea from "./InputArea";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";

vi.mock("../../utils/caretCoordinates", () => ({
    getCaretCoordinates: () => ({
        cursorLeft: 0,
        cursorTop: 0,
    }),
}));

function InputAreaHarness({
    initialText = "",
    planMode = false,
    planModeSwitching = false,
    onPlanModeToggle,
    isMobile = false,
    placement = "bottom",
    onSend = () => {},
    fileInfoList = null,
    onDeleteFile = () => {},
}: {
    initialText?: string;
    planMode?: boolean;
    planModeSwitching?: boolean;
    onPlanModeToggle?: () => void;
    isMobile?: boolean;
    placement?: "top" | "bottom";
    onSend?: () => void;
    fileInfoList?: import("../../data/Conversation").FileInfo[] | null;
    onDeleteFile?: (id: number) => void;
}) {
    const [inputText, setInputText] = useState(initialText);

    return (
        <InputArea
            inputText={inputText}
            setInputText={setInputText}
            fileInfoList={fileInfoList}
            handleChooseFile={() => {}}
            handlePaste={() => {}}
            handleDeleteFile={onDeleteFile}
            handleSend={onSend}
            aiIsResponsing={false}
            placement={placement}
            isMobile={isMobile}
            planMode={planMode}
            planModeSwitching={planModeSwitching}
            onPlanModeToggle={onPlanModeToggle}
        />
    );
}

describe("InputArea slash completion", () => {
    let bangList: [string, string, string, unknown?][];

    beforeEach(() => {
        bangList = [];
        clearAllMockHandlers();
        mockInvokeHandler("get_bang_list", () => bangList);
        mockInvokeHandler("get_assistants", () => []);
        mockInvokeHandler("get_artifacts_for_completion", () => []);
        mockInvokeHandler("get_skills_for_slash_completion", () => [
            {
                identifier: "agents:react-best-practices",
                displayName: "React Best Practices",
                invokeName: "React Best Practices",
                aliases: ["React Best Practices"],
                sourceType: "agents",
                sourceDisplayName: "Agents",
                description: "React coding conventions",
                tags: ["react", "frontend"],
            },
        ]);
    });

    it("shows slash namespaces after typing slash", async () => {
        const user = userEvent.setup();
        render(<InputAreaHarness />);

        const textarea = screen.getByRole("textbox");
        await user.type(textarea, "/");

        expect(await screen.findByText("skills")).toBeInTheDocument();
        expect(screen.getByText("artifacts")).toBeInTheDocument();
        expect(screen.getByText("即将支持")).toBeInTheDocument();
    });

    it("inserts skills invocation through slash selection", async () => {
        const user = userEvent.setup();
        render(<InputAreaHarness />);

        const textarea = screen.getByRole("textbox") as HTMLTextAreaElement;
        await user.type(textarea, "/");
        await user.keyboard("{Enter}");
        await new Promise((resolve) => setTimeout(resolve, 0));

        expect(textarea.value).toBe("/skills()");
        expect(textarea.selectionStart).toBe("/skills(".length);
    });

    it("replaces the skill query with the selected invoke name", async () => {
        const user = userEvent.setup();
        render(<InputAreaHarness initialText="/skills(react" />);
        await act(async () => {
            await new Promise((resolve) => setTimeout(resolve, 0));
        });

        const textarea = screen.getByRole("textbox") as HTMLTextAreaElement;
        act(() => {
            textarea.focus();
            textarea.setSelectionRange(textarea.value.length, textarea.value.length);
            document.dispatchEvent(new Event("selectionchange"));
        });

        expect(
            await screen.findByText("React coding conventions"),
        ).toBeInTheDocument();

        await user.keyboard("{Enter}");
        await new Promise((resolve) => setTimeout(resolve, 0));
        expect(textarea.value).toBe("/skills(React Best Practices)");
    });

    it("refreshes bang completions when plugin registry changes", async () => {
        const user = userEvent.setup();
        render(<InputAreaHarness />);

        await act(async () => {
            await new Promise((resolve) => setTimeout(resolve, 0));
        });

        bangList = [
            ["run_script", "run_script(|)", "执行一段 shell 命令，并将输出直接注入到提示词上下文。"],
        ];

        await act(async () => {
            await emit("plugin_registry_changed");
            await new Promise((resolve) => setTimeout(resolve, 0));
        });

        const textarea = screen.getByRole("textbox");
        await user.type(textarea, "!run");

        expect(
            await screen.findByText("执行一段 shell 命令，并将输出直接注入到提示词上下文。"),
        ).toBeInTheDocument();
    });
});

describe("InputArea Plan mode", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        mockInvokeHandler("get_bang_list", () => []);
        mockInvokeHandler("get_assistants", () => []);
        mockInvokeHandler("get_artifacts_for_completion", () => []);
        mockInvokeHandler("get_skills_for_slash_completion", () => []);
    });

    it("toggles Plan mode from the input toolbar", async () => {
        const onPlanModeToggle = vi.fn();
        const user = userEvent.setup();
        render(<InputAreaHarness onPlanModeToggle={onPlanModeToggle} />);

        const button = screen.getByRole("button", { name: "Plan" });
        expect(button).toHaveTextContent("Plan");
        expect(button).toHaveAttribute("aria-pressed", "false");
        expect(button).toHaveClass("input-area-plan-mode-button");
        expect(button).not.toHaveClass("bg-primary");
        expect(button).toHaveAttribute("data-aipp-slot", "chat-input-plan-mode");
        await user.click(button);
        expect(onPlanModeToggle).toHaveBeenCalledOnce();
    });

    it("fills the Plan chip with primary color while mode is on", () => {
        render(
            <InputAreaHarness
                planMode
                onPlanModeToggle={() => {}}
            />,
        );

        const button = screen.getByRole("button", { name: "Plan" });
        expect(button).toHaveTextContent("Plan");
        expect(button).toHaveAttribute("aria-pressed", "true");
        expect(button).toHaveClass("bg-primary");
        expect(button).toHaveClass("text-primary-foreground");
    });

    it("shows and disables the Plan control while switching", () => {
        render(
            <InputAreaHarness
                planMode
                planModeSwitching
                onPlanModeToggle={() => {}}
            />,
        );

        const button = screen.getByRole("button", { name: "Plan" });
        expect(button).toHaveAttribute("aria-pressed", "true");
        expect(button).toBeDisabled();
    });
});

describe("InputArea mobile vs desktop layout", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        mockInvokeHandler("get_bang_list", () => []);
        mockInvokeHandler("get_assistants", () => []);
        mockInvokeHandler("get_artifacts_for_completion", () => []);
        mockInvokeHandler("get_skills_for_slash_completion", () => []);
    });

    it("should insert newline instead of sending when Enter is pressed on mobile", () => {
        const onSend = vi.fn();
        render(<InputAreaHarness isMobile initialText="你好" onSend={onSend} />);
        const textarea = screen.getByRole("textbox");

        expect(textarea).toHaveAttribute("enterkeyhint", "enter");
        const event = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
        textarea.dispatchEvent(event);

        // 未 preventDefault：浏览器默认行为（插入换行）得以保留
        expect(event.defaultPrevented).toBe(false);
        expect(onSend).not.toHaveBeenCalled();
    });

    it("should send only via the send button on mobile", () => {
        const onSend = vi.fn();
        const { container } = render(<InputAreaHarness isMobile initialText="你好" onSend={onSend} />);
        const send = container.querySelector('[data-aipp-slot="chat-input-send-button"]') as HTMLElement;
        fireEvent.click(send);
        expect(onSend).toHaveBeenCalledOnce();
    });

    it("does not send when Enter confirms Chinese composition, then sends after the debounce", async () => {
        const onSend = vi.fn();
        render(<InputAreaHarness initialText="你好" onSend={onSend} />);
        const textarea = screen.getByRole("textbox");

        fireEvent.compositionStart(textarea);
        // WebKit may report isComposing=false even while confirming a candidate.
        fireEvent.keyDown(textarea, { key: "Enter", isComposing: false });
        fireEvent.compositionEnd(textarea, { data: "你好" });
        fireEvent.keyDown(textarea, { key: "Enter", isComposing: false });
        expect(onSend).not.toHaveBeenCalled();

        await act(async () => { await new Promise(resolve => setTimeout(resolve, 120)); });
        fireEvent.keyDown(textarea, { key: "Enter", isComposing: false });
        expect(onSend).toHaveBeenCalledOnce();
    });

    it("applies mobile class and keeps add/send slots without desktop inline right", () => {
        const { container } = render(<InputAreaHarness isMobile placement="bottom" />);

        const root = container.querySelector('[data-aipp-slot="chat-input-area"]');
        expect(root).toHaveClass("input-area");
        expect(root).toHaveClass("bottom");
        expect(root).toHaveClass("mobile");

        const textareaContainer = container.querySelector(
            '[data-aipp-slot="chat-input-area-container"]',
        ) as HTMLElement;
        expect(textareaContainer).toBeTruthy();
        // mobile must not carry desktop sidebar inline right
        expect(textareaContainer.style.right).toBe("");

        const add = container.querySelector(
            '[data-aipp-slot="chat-input-add-button"]',
        ) as HTMLElement;
        const send = container.querySelector(
            '[data-aipp-slot="chat-input-send-button"]',
        ) as HTMLElement;
        expect(add).toHaveClass("input-area-add-button");
        expect(send).toHaveClass("input-area-send-button");
        // CircleButton medium = h-8 w-8; CSS forces 44px send on mobile
        expect(send.className).toMatch(/\bh-8\b/);
        expect(send.className).not.toMatch(/\bh-14\b/);
        expect(add.style.right).toBe("");
        expect(send.style.right).toBe("");
    });

    it("keeps desktop wide layout: no mobile class, large send, inline right present", () => {
        const { container } = render(
            <InputAreaHarness isMobile={false} placement="bottom" />,
        );

        const root = container.querySelector('[data-aipp-slot="chat-input-area"]');
        expect(root).toHaveClass("input-area");
        expect(root).toHaveClass("bottom");
        expect(root).not.toHaveClass("mobile");

        const textareaContainer = container.querySelector(
            '[data-aipp-slot="chat-input-area-container"]',
        ) as HTMLElement;
        // desktop bottom uses inline right for sidebar clearance
        expect(textareaContainer.style.right).not.toBe("");

        const send = container.querySelector(
            '[data-aipp-slot="chat-input-send-button"]',
        ) as HTMLElement;
        expect(send.className).toMatch(/\bh-14\b/);
        expect(send.style.right).not.toBe("");
    });
});


describe("InputArea mobile attachment tray", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        mockInvokeHandler("get_bang_list", () => []);
        mockInvokeHandler("get_assistants", () => []);
        mockInvokeHandler("get_artifacts_for_completion", () => []);
        mockInvokeHandler("get_skills_for_slash_completion", () => []);
    });

    it("shows dedicated tray with count, text chip, and always-visible remove outside the capsule", () => {
        const files = [
            { id: 1, name: "shot.png", path: "/a.png", type: 1, thumbnail: "data:image/png;base64,aa" },
            { id: 2, name: "notes.txt", path: "/a.txt", type: 2 },
        ] as import("../../data/Conversation").FileInfo[];
        const { container } = render(
            <InputAreaHarness isMobile placement="bottom" fileInfoList={files} />,
        );

        const root = container.querySelector('[data-aipp-slot="chat-input-area"]');
        expect(root).toHaveClass("has-attachments");
        expect(root).toHaveAttribute("data-attachment-count", "2");

        expect(container.querySelector('[data-aipp-slot="chat-input-attachment-tray"]')).toBeTruthy();
        expect(screen.getByText("附件 2")).toBeTruthy();
        expect(screen.getByText("notes.txt")).toBeTruthy();
        expect(screen.getByText("TXT")).toBeTruthy();

        const capsule = container.querySelector('[data-aipp-slot="chat-input-area-container"]');
        expect(capsule?.querySelector('[data-aipp-slot="chat-input-attachments"]')).toBeNull();

        const removes = container.querySelectorAll('[data-aipp-slot="chat-input-attachment-remove"]');
        expect(removes.length).toBe(2);
    });
});
