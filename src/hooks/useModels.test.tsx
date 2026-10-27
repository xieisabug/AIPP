import { act, renderHook, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { beforeEach, expect, it } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { useModels } from "./useModels";
import { useFilteredProviders } from "./useFilteredProviders";

beforeEach(() => clearAllMockHandlers());

it("should reload models and provider choices when their sync domain changes", async () => {
    let modelId = 1;
    mockInvokeHandler("get_models_for_select", () => [{ id: modelId, code: `model-${modelId}`, llm_provider_id: 1 }]);
    mockInvokeHandler("get_filtered_providers", () => [{ id: modelId, name: `provider-${modelId}` }]);
    const { result } = renderHook(() => ({ models: useModels(), providers: useFilteredProviders(4) }));
    await waitFor(() => expect(result.current.models.models[0]?.id).toBe(1));
    modelId = 2;
    await act(async () => { await emit("llm_provider_changed"); });
    await waitFor(() => expect(result.current.models.models[0]?.id).toBe(2));
    expect(result.current.providers.providers[0].id).toBe(2);
});
