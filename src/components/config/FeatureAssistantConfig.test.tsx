import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import FeatureAssistantConfig from "@/components/config/FeatureAssistantConfig";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";

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
                        custom_headers: "{}",
                    },
                })
            );
        });
    });
});
