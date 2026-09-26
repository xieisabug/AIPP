import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import FeatureAssistantConfig from "@/components/config/FeatureAssistantConfig";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";

const platformState = vi.hoisted(() => ({ value: "windows" }));

vi.mock("@/hooks/use-platform", () => ({
    usePlatform: () => platformState.value,
    useIsMobilePlatform: () => platformState.value === "android" || platformState.value === "ios",
}));

vi.mock("@/hooks/feature/useVersionManager", () => ({
    useVersionManager: () => ({
        bunVersion: "",
        uvVersion: "",
        isInstallingBun: false,
        isInstallingUv: false,
        bunInstallLog: "",
        uvInstallLog: "",
        installBun: vi.fn(),
        installUv: vi.fn(),
        bunLatestVersion: null,
        uvLatestVersion: null,
        isCheckingBunUpdate: false,
        isCheckingUvUpdate: false,
        isUpdatingBun: false,
        isUpdatingUv: false,
        checkBunUpdate: vi.fn(),
        checkUvUpdate: vi.fn(),
        updateBun: vi.fn(),
        updateUv: vi.fn(),
        python2Version: "",
        python3Version: "",
        installedPythons: [],
        needInstallPython3: false,
        isInstallingPython: false,
        pythonInstallLog: "",
        checkPythonVersions: vi.fn(),
        installPython3: vi.fn(),
    }),
}));

vi.mock("@/services/PluginRuntime", () => ({
    pluginRuntime: {
        loadPlugins: vi.fn(async () => undefined),
        listDisplayThemes: vi.fn(async () => []),
    },
}));

describe("FeatureAssistantConfig network config", () => {
    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
        window.innerWidth = 1024;
        platformState.value = "windows";
    });

    it("saves network_proxy into global network_config", async () => {
        window.innerWidth = 1400;
        window.dispatchEvent(new Event("resize"));

        const featureRows = [
            { id: 1, feature_code: "network_config", key: "request_timeout", value: "180" },
            { id: 2, feature_code: "network_config", key: "retry_attempts", value: "3" },
            { id: 3, feature_code: "network_config", key: "network_proxy", value: "" },
            { id: 4, feature_code: "network_config", key: "custom_headers", value: "{}" },
        ];
        mockInvokeHandler("get_all_feature_config", () => featureRows);
        mockInvokeHandler("save_feature_config", () => undefined);
        mockInvokeHandler("list_syntect_themes", () => []);
        mockInvokeHandler("get_enabled_plugins", () => []);

        render(<FeatureAssistantConfig />);

        const user = userEvent.setup();
        await user.click(await screen.findByRole("button", { name: /网络配置/ }));
        const proxyInput = await screen.findByPlaceholderText("http://127.0.0.1:7890");
        fireEvent.change(proxyInput, {
            target: { value: "http://proxy.example.com:8080" },
        });
        await user.click(screen.getByRole("button", { name: "保存配置" }));

        await waitFor(() => {
            const saveCalls = vi.mocked(invoke).mock.calls.filter(([command]) => command === "save_feature_config");
            expect(saveCalls).toHaveLength(1);
            expect(saveCalls[0][1]).toEqual(
                expect.objectContaining({
                    featureCode: "network_config",
                    feature_code: "network_config",
                    config: {
                        request_timeout: "180",
                        retry_attempts: "3",
                        network_proxy: "http://proxy.example.com:8080",
                        openai_prompt_cache_key_enabled: "true",
                        openai_prompt_cache_retention: "24h",
                        openai_responses_stateful_enabled: "false",
                        custom_headers: "{}",
                    },
                })
            );
        });
    });

    it("hides the shortcuts settings on Android", async () => {
        platformState.value = "android";
        window.innerWidth = 1400;
        window.dispatchEvent(new Event("resize"));

        mockInvokeHandler("get_all_feature_config", () => []);
        mockInvokeHandler("list_syntect_themes", () => []);
        mockInvokeHandler("get_enabled_plugins", () => []);

        render(<FeatureAssistantConfig />);

        await screen.findByRole("button", { name: /网络配置/ });
        expect(screen.queryByRole("button", { name: /快捷键/ })).not.toBeInTheDocument();
    });

    it("should save an empty auto review model when auxiliary AI config is saved", async () => {
        window.innerWidth = 1400;
        window.dispatchEvent(new Event("resize"));

        mockInvokeHandler("get_all_feature_config", () => [
            { id: 1, feature_code: "conversation_summary", key: "assistant_ai_enabled", value: "true" },
            { id: 2, feature_code: "conversation_summary", key: "title_summary_enabled", value: "true" },
            { id: 3, feature_code: "conversation_summary", key: "title_model", value: "title-model" },
            { id: 4, feature_code: "conversation_summary", key: "title_provider_id", value: "2" },
            { id: 5, feature_code: "conversation_summary", key: "title_summary_length", value: "100" },
            { id: 6, feature_code: "conversation_summary", key: "auto_review_model", value: "" },
            { id: 7, feature_code: "conversation_summary", key: "auto_review_provider_id", value: "" },
        ]);
        mockInvokeHandler("save_feature_config", () => undefined);
        mockInvokeHandler("get_models_for_select", () => []);
        mockInvokeHandler("list_syntect_themes", () => []);
        mockInvokeHandler("get_enabled_plugins", () => []);

        render(<FeatureAssistantConfig />);

        const user = userEvent.setup();
        await user.click(await screen.findByRole("button", { name: /辅助AI/ }));
        await user.click(await screen.findByRole("button", { name: "保存配置" }));

        await waitFor(() => {
            const saveCalls = vi.mocked(invoke).mock.calls.filter(([command, args]) => {
                const payload = args as { featureCode?: string } | undefined;
                return command === "save_feature_config" && payload?.featureCode === "conversation_summary";
            });
            expect(saveCalls).toHaveLength(1);
            expect(saveCalls[0][1]).toEqual(
                expect.objectContaining({
                    featureCode: "conversation_summary",
                    config: expect.objectContaining({
                        auto_review_model: "",
                        auto_review_provider_id: "",
                        title_model: "title-model",
                        title_provider_id: "2",
                    }),
                })
            );
        });
    });
});
