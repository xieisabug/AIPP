import { act, renderHook, waitFor } from "@testing-library/react";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { AssistantListItem } from "@/data/Assistant";
import { useAssistantListListener } from "./useAssistantListListener";

describe("useAssistantListListener", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    afterEach(() => {
        vi.restoreAllMocks();
    });

    it("keeps a single listener registration and dispatches to the latest callback", async () => {
        const assistantList: AssistantListItem[] = [
            {
                id: 7,
                name: "Current Assistant",
                assistant_type: 0,
            },
        ];
        mockInvokeHandler("get_assistants", () => assistantList);

        let registeredHandler: (() => Promise<void>) | null = null;
        vi.mocked(listen).mockImplementation(async (_event, handler) => {
            registeredHandler = async () => {
                await handler({
                    event: "assistant_list_changed",
                    id: 1,
                    payload: undefined,
                });
            };
            return () => {
                registeredHandler = null;
            };
        });

        const firstCallback = vi.fn();
        const secondCallback = vi.fn();

        const { rerender } = renderHook(
            ({ onAssistantListChanged }) => useAssistantListListener({ onAssistantListChanged }),
            {
                initialProps: {
                    onAssistantListChanged: firstCallback,
                },
            }
        );

        await waitFor(() => {
            expect(listen).toHaveBeenCalledTimes(1);
        });

        rerender({
            onAssistantListChanged: secondCallback,
        });

        await waitFor(() => {
            expect(listen).toHaveBeenCalledTimes(1);
        });

        await act(async () => {
            await registeredHandler?.();
        });

        await waitFor(() => {
            expect(secondCallback).toHaveBeenCalledWith(assistantList);
        });
        expect(firstCallback).not.toHaveBeenCalled();
    });
});
