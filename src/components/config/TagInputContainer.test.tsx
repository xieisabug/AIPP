import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import TagInputContainer from "./TagInputContainer";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), error: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("sonner", () => ({ toast: { error: mocks.error } }));
vi.mock("../TagInput", () => ({ default: ({ onFetchModels }: { onFetchModels: () => void }) => <button onClick={onFetchModels}>获取模型</button> }));

describe("provider model request validation", () => {
    beforeEach(() => { mocks.invoke.mockReset(); mocks.error.mockReset(); });

    it("should prevent a models request when the current endpoint draft is invalid", async () => {
        render(<TagInputContainer llmProviderId="1" apiType="openai_api" tags={[]} onTagsChange={vi.fn()}
            onFetchModels={vi.fn()} beforeFetchModels={() => { throw new Error("Endpoint 无效"); }} />);
        fireEvent.click(screen.getByRole("button", { name: "获取模型" }));
        await waitFor(() => expect(mocks.error).toHaveBeenCalledWith(expect.stringContaining("Endpoint 无效")));
        expect(mocks.invoke).not.toHaveBeenCalled();
    });

    it("should use the latest draft validator when memoized props change", async () => {
        const onTagsChange = vi.fn();
        const onFetchModels = vi.fn();
        const props = { llmProviderId: "1", apiType: "openai_api", tags: [], onTagsChange, onFetchModels };
        const { rerender } = render(<TagInputContainer {...props} beforeFetchModels={() => undefined} />);
        rerender(<TagInputContainer {...props} beforeFetchModels={() => { throw new Error("Endpoint 无效"); }} />);
        fireEvent.click(screen.getByRole("button", { name: "获取模型" }));
        await waitFor(() => expect(mocks.error).toHaveBeenCalled());
        expect(mocks.invoke).not.toHaveBeenCalled();
    });

    it("should return models when the current endpoint draft passes validation", async () => {
        const data = { available_models: [], missing_models: [] };
        mocks.invoke.mockResolvedValue(data);
        const onFetchModels = vi.fn();
        render(<TagInputContainer llmProviderId="1" apiType="openai_api" tags={[]} onTagsChange={vi.fn()}
            onFetchModels={onFetchModels} beforeFetchModels={() => undefined} />);
        fireEvent.click(screen.getByRole("button", { name: "获取模型" }));
        await waitFor(() => expect(onFetchModels).toHaveBeenCalledWith(data));
        expect(mocks.invoke).toHaveBeenCalledWith("preview_model_list", { llmProviderId: 1 });
    });
});
