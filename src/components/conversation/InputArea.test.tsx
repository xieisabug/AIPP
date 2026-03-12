import { useState } from "react";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import InputArea from "./InputArea";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";

vi.mock("../../utils/caretCoordinates", () => ({
    getCaretCoordinates: () => ({
        cursorLeft: 0,
        cursorTop: 0,
    }),
}));

function InputAreaHarness({ initialText = "" }: { initialText?: string }) {
    const [inputText, setInputText] = useState(initialText);

    return (
        <InputArea
            inputText={inputText}
            setInputText={setInputText}
            fileInfoList={null}
            handleChooseFile={() => {}}
            handlePaste={() => {}}
            handleDeleteFile={() => {}}
            handleSend={() => {}}
            aiIsResponsing={false}
        />
    );
}

describe("InputArea slash completion", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        mockInvokeHandler("get_bang_list", () => []);
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
});
