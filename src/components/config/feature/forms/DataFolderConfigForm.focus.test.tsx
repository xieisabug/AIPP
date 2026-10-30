import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { useForm } from "react-hook-form";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";

import { DataFolderConfigForm } from "./DataFolderConfigForm";

vi.mock("sonner", () => ({
    toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));

function Harness() {
    const form = useForm({ defaultValues: { mode: "self_hosted", server_url: "", token: "" } });
    return <DataFolderConfigForm form={form} />;
}

const viewports = [
    { name: "desktop", width: 1400 },
    { name: "mobile", width: 390 },
];

describe.each(viewports)("DataFolderConfigForm server URL field ($name)", ({ width }) => {
    beforeEach(() => {
        window.innerWidth = width;
        window.dispatchEvent(new Event("resize"));
        mockInvokeHandler("get_sync_status", () => ({
            mode: "self_hosted", server_url: "", token_configured: false, connected: false,
            running: false, syncing: false, paused: false, last_sync_at: null, last_error: null,
            pending_outbox_count: 0, pushing_outbox_count: 0, failed_outbox_count: 0,
            dead_letter_count: 0, needs_reset: false, conflict_resolved_count: 0, server_cursor: 0,
            dangling_fk_count: 0,
        }));
    });
    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
        window.innerWidth = 1024;
        window.dispatchEvent(new Event("resize"));
    });

    it("keeps focus while a full URL is typed (the form re-renders on every character)", async () => {
        const user = userEvent.setup();
        render(<Harness />);
        await waitFor(() => expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "get_sync_status")).toBe(true));
        const input = screen.getByPlaceholderText("https://sync.example.com") as HTMLInputElement;
        await user.click(input);
        await user.type(input, "http://127.0.0.1:18765");
        expect(screen.getByPlaceholderText("https://sync.example.com")).toBe(input);
        expect(document.activeElement).toBe(input);
        expect(input.value).toBe("http://127.0.0.1:18765");
        // The plaintext hint is derived from the watched value; it re-renders the form.
        expect(screen.getByText(/明文传输/)).toBeTruthy();
    });
});
