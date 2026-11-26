import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { beforeEach, describe, it, vi, expect } from "vitest";
import { invoke, mockInvokeHandler, clearAllMockHandlers } from "@/__tests__/mocks/tauri";
import type { Config, OnboardingState, Provider } from "@/components/onboarding/onboardingService";
import OnboardingWindow from "./OnboardingWindow";
vi.mock("@tauri-apps/api/core", async () => ({
    ...(await import("@/__tests__/mocks/tauri")),
    Channel: class {
        onmessage = () => {};
    },
}));
vi.mock("@/components/avatar/AnimatedAvatar", () => ({ default: () => null }));
vi.mock("@/components/window/WindowTitleBar", () => ({ WindowTopBar: () => null }));
vi.mock("@/hooks/useTheme", () => ({ useTheme: () => {} }));
vi.mock("@/hooks/useVaultItems", () => ({ useVaultItems: () => ({ available: false, items: [] }) }));
const statuses: Record<string, { state: string }> = {
    claude_sdk: { state: "installed" },
    codex_app_server: { state: "installed" },
    pi: { state: "installed" },
    cursor: { state: "installed" },
};
vi.mock("@/components/config/useLocalAgentDetection", () => ({
    useLocalAgentDetection: () => ({ statuses, acpClis: [] }),
}));
const preset = {
    recommended_provider: {
        api_type: "deepseek",
        name: "DeepSeek",
        endpoint: "https://example.test",
        price_hint: "",
        key_guide: [],
        models: { main: "main", light: "small" },
    },
    recommendation_catalog: {providers:{deepseek:{endpoints:["https://fixture.test"], models:{
        main:{model:"lab/main",input:2,output:2},small:{model:"lab/small",input:0.2,output:0.2},
    }}}, benchmarks:{
        "lab/main":[{group:"reasoning",test:"reason",score:90},{group:"unsafeAllowance",test:"risk",score:30,lowerBetter:true}],
        "lab/small":[{group:"reasoning",test:"reason",score:40},{group:"unsafeAllowance",test:"risk",score:3,lowerBetter:true}],
    }},
    model_ranking: { main: [], light: [] },
    strip_suffixes: [],
    light_keywords: [],
};
function setup() {
    let state: OnboardingState = {
        state: null,
        path: null,
        step: null,
        workspacePath: "C:/AIPP",
        workspaceLocked: false,
        canOpen: true,
        rerun: false,
        activeLight: null,
        activeReview: null,
        draft: { providers: [] },
    };
    let provider: Provider | undefined;
    let balance = 5;
    const cfg: Record<string, string> = {};
    const configure = (patch: Partial<OnboardingState>) => {
        state = { ...state, ...patch };
    };
    mockInvokeHandler("get_onboarding_state", () => state);
    mockInvokeHandler("get_onboarding_preset", () => preset);
    mockInvokeHandler("onboarding_provider_endpoints", () => ({6:"https://fixture.test", 17:"https://fixture.test",44:"https://fixture.test"}));
    mockInvokeHandler("get_onboarding_setup_status", () => [{ id: "bun", state: "running", message: "正在下载" }]);
    mockInvokeHandler("get_all_feature_config", () => []);
    mockInvokeHandler("get_llm_providers", () => (provider ? [provider] : []));
    mockInvokeHandler("get_assistant", () => ({ model: [] }));
    mockInvokeHandler("get_filtered_models_for_select", () => []);
    mockInvokeHandler("choose_onboarding_path", (args) =>
        configure({ state: "in_progress", path: args!.path as OnboardingState["path"], step: "path_chosen" })
    );
    mockInvokeHandler("add_llm_provider", (args) => {
        provider = { id: 44, name: String(args?.name), api_type: String(args?.apiType), is_enabled: false };
        return 44;
    });
    mockInvokeHandler("update_llm_provider_config", (args) => {
        cfg[String(args?.name)] = String(args?.value);
    });
    mockInvokeHandler("get_llm_provider_config", () =>
        Object.entries(cfg).map(([name, value]) => ({ name, value }))
    );
    mockInvokeHandler("track_onboarding_provider", () => undefined);
    mockInvokeHandler("get_deepseek_balance", () => ({ available: balance > 0, total: String(balance) }));
    mockInvokeHandler("save_onboarding_recharge", () =>
        configure({
            step: "awaiting_recharge",
            draft: { providers: [{ providerId: 44, owned: true }], rechargeProviderId: 44 },
        })
    );
    mockInvokeHandler("preview_model_list", () => ({
        available_models: ["main", "small"].map((code) => ({ code, request_mode: "chat_completions" })),
    }));
    mockInvokeHandler("probe_chat_model", (args) =>
        args?.modelCode === "small"
            ? { ok: false, kind: "rate_limited", message: "429" }
            : { ok: true, kind: "ok", message: "" }
    );
    mockInvokeHandler("check_onboarding_agent", () => ["agent-small", "agent-large"]);
    mockInvokeHandler("read_local_agent_defaults", () => ({ settings: {} }));
    mockInvokeHandler("get_provider_quota", () => ({ windows: [] }));
    mockInvokeHandler("apply_onboarding_config", (args) => {
        const config = args!.config as Config;
        configure({ state: "completed", step: "applied", draft: { providers: [], config } });
        return state;
    });
    mockInvokeHandler("finish_onboarding", () => undefined);
    mockInvokeHandler("update_onboarding_roles", (args) => {
        const config = { ...state.draft.config!, ...args };
        if (args?.clearLight) { config.light = null; delete config.clearLight; }
        if (args?.clearReview) { config.review = null; delete config.clearReview; }
        configure({
            activeLight: config.light?.verified ? `${config.light.code}%%${config.light.providerId}` : null,
            activeReview: config.review?.verified ? `${config.review.code}%%${config.review.providerId}` : null,
            draft: { ...state.draft, config },
        });
        return state;
    });
    return {
        configure,
        balance: (value: number) => {
            balance = value;
        },
    };
}
describe("first-run window", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        invoke.mockClear();
    });
    it("should resume a failed role-only save in the current window", async () => {
        const backend = setup();
        const model = { providerId: 6, code: "main", verified: true };
        const config: Config = { chat: model, light: model, review: model, home: "chat_ui", extras: [] };
        backend.configure({
            state: "completed",
            step: "applied",
            path: "expert",
            activeLight: "main%%6",
            activeReview: "main%%6",
            draft: { providers: [], config },
        });
        mockInvokeHandler("get_llm_providers", () => [{ id: 6, name: "Ollama", api_type: "ollama", is_enabled: true }]);
        mockInvokeHandler("get_filtered_models_for_select", (args) =>
            args?.assistantType === 4 ? [] : [{ id: 1, llm_provider_id: 6, code: "main", name: "main" }]
        );
        mockInvokeHandler("update_onboarding_roles", () => {
            backend.configure({
                step: "source_ready",
                draft: {
                    providers: [],
                    config: { ...config, light: null, review: null, clearLight: true, clearReview: true },
                },
            });
            throw new Error("配置数据库暂时被占用");
        });
        render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: "换一个" }));
        fireEvent.change(await screen.findByLabelText("轻量模型"), { target: { value: "" } });
        fireEvent.click(screen.getByRole("button", { name: "应用" }));
        fireEvent.click(await screen.findByRole("button", { name: "重试应用" }));
        await screen.findByRole("button", { name: "打开聊天" });
        expect(invoke).toHaveBeenCalledWith("apply_onboarding_config", {
            config: expect.objectContaining({ clearLight: true, clearReview: true }),
        });
        expect(invoke.mock.calls.filter(([name]) => name === "probe_chat_model")).toHaveLength(0);
    });
    it("should keep the untouched review setting when an expert rerun explicitly clears light", async () => {
        const backend = setup();
        backend.configure({
            state: "completed",
            step: "path_chosen",
            path: "expert",
            rerun: true,
            activeLight: "old-small%%6",
            activeReview: "old-review%%6",
        });
        mockInvokeHandler("get_llm_providers", () => [{ id: 6, name: "Ollama", api_type: "ollama", is_enabled: true }]);
        mockInvokeHandler("get_filtered_models_for_select", (args) =>
            args?.assistantType === 4
                ? []
                : [
                      {
                          id: 1,
                          llm_provider_id: 6,
                          code: "main",
                          name: "main",
                      },
                  ]
        );
        mockInvokeHandler("get_office_preview_plugin_status", () => ({ installed: false }));
        mockInvokeHandler("check_bun_version", () => "Not Installed");
        mockInvokeHandler("check_uv_version", () => "Not Installed");
        render(<OnboardingWindow />);
        const next = await screen.findByRole("button", { name: "下一步" });
        await waitFor(() => expect(next).toBeEnabled());
        fireEvent.click(next);
        const primary = await screen.findByLabelText("新对话默认使用");
        await waitFor(() => expect(primary.querySelector("option[value='main%%6']")).not.toBeNull());
        fireEvent.change(primary, { target: { value: "main%%6" } });
        expect(screen.getByLabelText("轻量模型")).toHaveValue("keep");
        expect(screen.getByLabelText("审核模型")).toHaveValue("keep");
        fireEvent.change(screen.getByLabelText("轻量模型"), { target: { value: "" } });
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        await screen.findByRole("heading", { name: "选择主界面" });
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        const complete = await screen.findByRole("button", { name: "完成" });
        await waitFor(() => expect(complete).toBeEnabled());
        fireEvent.click(complete);
        fireEvent.click(await screen.findByRole("button", { name: "确认应用" }));
        await screen.findByRole("button", { name: "打开聊天" });
        const submitted = invoke.mock.calls.find(([name]) => name === "apply_onboarding_config")?.[1] as {
            config: Config;
        };
        expect(submitted.config.clearLight).toBe(true);
        expect(submitted.config.clearReview).toBeUndefined();
        expect(submitted.config.review).toBeUndefined();
    });
    it.each([false, true])(
        "should clear only the explicitly removed role when changing the primary model is %s",
        async (changePrimary) => {
            const backend = setup();
            const model = (code: string) => ({ providerId: 6, code, verified: true });
            const config: Config = {
                chat: model("main"),
                light: model("light"),
                review: model("review"),
                home: "chat_ui",
                extras: [],
            };
            backend.configure({
                state: "completed",
                step: "applied",
                path: "expert",
                activeLight: "light%%6",
                activeReview: "review%%6",
                draft: { providers: [], config },
            });
            mockInvokeHandler("get_llm_providers", () => [
                { id: 6, name: "Ollama", api_type: "ollama", is_enabled: true },
            ]);
            mockInvokeHandler("get_filtered_models_for_select", (args) =>
                args?.assistantType === 4
                    ? []
                    : ["main", "other", "light", "review"].map((code, i) => ({
                          id: i + 1,
                          llm_provider_id: 6,
                          code,
                          name: code,
                      }))
            );
            render(<OnboardingWindow />);
            fireEvent.click(await screen.findByRole("button", { name: "换一个" }));
            if (changePrimary)
                fireEvent.change(await screen.findByLabelText("新对话默认使用"), { target: { value: "other%%6" } });
            fireEvent.change(await screen.findByLabelText("审核模型"), { target: { value: "" } });
            fireEvent.click(screen.getByRole("button", { name: "应用" }));
            await screen.findByRole("button", { name: "打开聊天" });
            if (changePrimary) {
                expect(invoke).toHaveBeenCalledWith("apply_onboarding_config", {
                    config: expect.objectContaining({
                        chat: model("other"),
                        light: model("light"),
                        review: null,
                        clearReview: true,
                    }),
                });
            } else {
                expect(invoke).toHaveBeenCalledWith("update_onboarding_roles", { clearReview: true });
                expect(invoke.mock.calls.filter(([name]) => name === "probe_chat_model")).toHaveLength(0);
                fireEvent.click(screen.getByRole("button", { name: "换一个" }));
                expect(await screen.findByLabelText("审核模型")).toHaveValue("");
            }
        }
    );
    async function selectRecommendedAndApply() {
        await screen.findByRole("heading", { name: "选择模型" });
        expect(invoke.mock.calls.some(([cmd]) => cmd === "probe_chat_model")).toBe(false);
        const callsBeforePick = invoke.mock.calls.length;
        fireEvent.click(screen.getByRole("button", { name: "帮我选" }));
        expect(invoke.mock.calls).toHaveLength(callsBeforePick);
        const apply = screen.getByRole("button", { name: "应用" });
        await waitFor(() => expect(apply).toBeEnabled());
        expect(invoke.mock.calls.some(([cmd]) => cmd === "apply_onboarding_config")).toBe(false);
        fireEvent.click(apply);
    }
    it("来源页点下一步可浏览 13 个模型、返回并手动选择，应用时直接保存所选型号", async () => {
        setup();
        const codes = Array.from({ length: 13 }, (_, index) => `chat-${index + 1}`);
        mockInvokeHandler("preview_model_list", () => ({
            available_models: codes.map(code => ({ code, request_mode: "chat_completions" })),
        }));
        render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: /躺平/ }));
        fireEvent.click(await screen.findByRole("button", { name: /我用的是其他服务商/ }));
        fireEvent.click(await screen.findByRole("button", { name: /OpenAI API/ }));
        fireEvent.change(await screen.findByLabelText("API Key"), { target: { value: "synthetic" } });
        fireEvent.click(screen.getByRole("button", { name: "完成" }));
        await screen.findByText("已获取 13 个模型");
        expect(screen.queryByRole("button", { name: "帮我选" })).not.toBeInTheDocument();
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        const list = await screen.findByLabelText("主力模型");
        expect([...list.querySelectorAll("option")].map(option => option.value).filter(Boolean)).toEqual(codes);
        expect(screen.getByRole("button", { name: "应用" })).toBeDisabled();
        fireEvent.click(screen.getByRole("button", { name: "上一步" }));
        await screen.findByText("已获取 13 个模型");
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        expect(await screen.findByRole("combobox", { name: "主力模型" })).toBeVisible();
        fireEvent.change(screen.getByLabelText("主力模型"), { target: { value: "chat-13" } });
        fireEvent.change(screen.getByLabelText("轻量模型"), { target: { value: "chat-7" } });
        fireEvent.change(screen.getByLabelText("审核模型"), { target: { value: "chat-7" } });
        expect(invoke.mock.calls.some(([cmd]) => cmd === "probe_chat_model")).toBe(false);
        fireEvent.click(screen.getByRole("button", { name: "应用" }));
        await screen.findByRole("heading", { name: "好了，可以开始了" });
        expect(invoke.mock.calls.filter(([cmd]) => cmd === "preview_model_list")).toHaveLength(1);
        expect(invoke.mock.calls.filter(([cmd]) => cmd === "probe_chat_model")).toHaveLength(0);
        expect(invoke).toHaveBeenCalledWith("apply_onboarding_config", { config: expect.objectContaining({
            chat: { providerId: 44, code: "chat-13", verified: true },
            light: { providerId: 44, code: "chat-7", verified: true },
            review: { providerId: 44, code: "chat-7", verified: true },
        }) });
    });
    it("只在点击测试时请求当前角色，切换型号后忽略旧结果，测试失败仍能应用", async () => {
        setup();
        let finishOld!: (value: { ok: boolean; message: string }) => void;
        mockInvokeHandler("probe_chat_model", args => args?.modelCode === "main"
            ? new Promise(resolve => { finishOld = resolve; })
            : { ok: false, kind: "rate_limited", message: "429 额度限制" });
        render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: /躺平/ }));
        fireEvent.change(await screen.findByLabelText("API Key"), { target: { value: "synthetic" } });
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        const select = await screen.findByLabelText("主力模型");
        const test = () => screen.getByRole("button", { name: "测试主力模型" });
        expect(test()).toBeDisabled();
        fireEvent.change(select, { target: { value: "main" } });
        fireEvent.click(test());
        expect(test()).toBeDisabled();
        fireEvent.change(select, { target: { value: "small" } });
        fireEvent.click(test());
        expect(await screen.findByRole("alert")).toHaveTextContent("429 额度限制");
        await act(async () => finishOld({ ok: true, message: "" }));
        expect(screen.queryByText("测试成功")).toBeNull();
        fireEvent.click(screen.getByRole("button", { name: "应用" }));
        await screen.findByRole("heading", { name: "好了，可以开始了" });
        expect(invoke.mock.calls.filter(([cmd]) => cmd === "probe_chat_model").map(([, args]) => args))
            .toEqual([{ providerId: 44, modelCode: "main" }, { providerId: 44, modelCode: "small" }]);
    });
    it("点击本机轻量模型的测试按钮时，通过该实例的原生通道发送请求", async () => {
        setup();
        mockInvokeHandler("test_onboarding_agent_model", () => null);
        render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: /躺平/ }));
        fireEvent.click(await screen.findByRole("button", { name: "Codex · 已安装" }));
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        fireEvent.change(await screen.findByLabelText("轻量模型"), { target: { value: "agent-small%%44" } });
        expect(invoke.mock.calls.some(([cmd]) => cmd === "test_onboarding_agent_model")).toBe(false);
        fireEvent.click(screen.getByRole("button", { name: "测试轻量模型" }));
        expect(await screen.findByText("测试成功")).toBeVisible();
        fireEvent.click(screen.getByRole("button", { name: "应用" }));
        await screen.findByRole("heading", { name: "好了，可以开始了" });
        expect(invoke.mock.calls.filter(([cmd]) => cmd === "test_onboarding_agent_model").map(([, args]) => args))
            .toEqual([{ providerId: 44, modelCode: "agent-small" }]);
        expect(invoke.mock.calls.some(([cmd]) => cmd === "probe_chat_model")).toBe(false);
    });
    it("躺平模式直接保存网关目录中的模型，不探测聊天协议", async () => {
        setup();
        mockInvokeHandler("preview_model_list", () => ({
            available_models: [{ code: "gateway-model", request_mode: "chat_completions" }],
        }));
        render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: /躺平/ }));
        fireEvent.click(await screen.findByRole("button", { name: /我用的是其他服务商/ }));
        fireEvent.click(await screen.findByRole("button", { name: /OpenAI API/ }));
        fireEvent.change(await screen.findByLabelText("API Key"), { target: { value: "synthetic" } });
        fireEvent.click(screen.getByRole("button", { name: "完成" }));
        await screen.findByText("已获取 1 个模型");
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        fireEvent.change(await screen.findByLabelText("主力模型"), { target: { value: "gateway-model" } });
        fireEvent.click(screen.getByRole("button", { name: "应用" }));
        await screen.findByRole("heading", { name: "好了，可以开始了" });
        expect(invoke.mock.calls.some(([cmd]) => cmd === "probe_chat_model" || cmd === "update_llm_model_request_mode")).toBe(false);
    });
    it.each([false, true])("躺平添加提供商只获取列表，应用选定模型不试调用（首次列表失败：%s）", async (listFailsOnce) => {
        setup();
        if (listFailsOnce) {
            let first = true;
            mockInvokeHandler("preview_model_list", () => {
                if (first) {
                    first = false;
                    throw { kind: "network", message: "连接中断" };
                }
                return { available_models: [{ code: "main", request_mode: "chat_completions" }] };
            });
        }
        render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: /躺平/ }));
        fireEvent.click(await screen.findByRole("button", { name: /我用的是其他服务商/ }));
        fireEvent.click(await screen.findByRole("button", { name: /OpenAI API/ }));
        fireEvent.change(await screen.findByLabelText("API Key"), { target: { value: "synthetic" } });
        fireEvent.click(screen.getByRole("button", { name: "完成" }));
        if (listFailsOnce) await screen.findByText("连不上 OpenAI API，检查网络或代理设置");
        else await screen.findByText("已获取 2 个模型");
        expect(invoke.mock.calls.some(([cmd]) => cmd === "probe_chat_model" || cmd === "apply_onboarding_config")).toBe(false);
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        await selectRecommendedAndApply();
        await screen.findByRole("heading", { name: "好了，可以开始了" });
        expect(invoke.mock.calls.filter(([cmd]) => cmd === "preview_model_list")).toHaveLength(listFailsOnce ? 2 : 1);
        expect(invoke.mock.calls.filter(([cmd]) => cmd === "add_llm_provider")).toHaveLength(1);
        expect(invoke.mock.calls.some(([cmd]) => cmd === "probe_chat_model")).toBe(false);
    });
    it("躺平分享码在下一步导入，选模后直接保存", async () => {
        setup();
        mockInvokeHandler("import_llm_provider", () => ({ id: 17, api_type: "openai_api", name: "示例", is_enabled: false }));
        render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: /躺平/ }));
        fireEvent.click(await screen.findByRole("button", { name: /有人发给我分享码/ }));
        const code = btoa(JSON.stringify({ type: "provider", encrypted_data: "fixture", nonce: "test" }));
        fireEvent.change(screen.getByLabelText("分享消息"), { target: { value: `分享码：${code}\n密码：example` } });
        const select = screen.getByRole("button", { name: "下一步" });
        expect(select).toBeEnabled();
        expect(invoke.mock.calls.some(([cmd]) => cmd === "import_llm_provider" || cmd === "probe_chat_model")).toBe(false);
        fireEvent.click(select);
        await selectRecommendedAndApply();
        await screen.findByRole("heading", { name: "好了，可以开始了" });
        expect(invoke.mock.calls.some(([cmd]) => cmd === "probe_chat_model")).toBe(false);
    });
    it("躺平保留全部型号，审核默认 Flash，点击帮我选后使用角色评分", async () => {
        setup();
        const codes = ["small", "deepseek-flash", "main", "embedding-test"];
        mockInvokeHandler("preview_model_list", () => ({
            available_models: codes.map(code => ({
                code, request_mode: code === "embedding-test" ? "unsupported" : "chat_completions",
            })),
        }));
        render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: /躺平/ }));
        fireEvent.change(await screen.findByLabelText("API Key"), { target: { value: "synthetic" } });
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        expect(await screen.findByLabelText("审核模型")).toHaveValue("deepseek-flash");
        fireEvent.click(screen.getByRole("button", {name:"帮我选"}));
        expect(screen.getByLabelText("审核模型")).toHaveValue("small");
        fireEvent.click(screen.getByRole("button", {name:"应用"}));
        await screen.findByRole("heading", { name: "好了，可以开始了" });
        expect(invoke).toHaveBeenCalledWith("apply_onboarding_config", { config: expect.objectContaining({
            models: [{ providerId: 44, codes }],
            review: { providerId: 44, code: "small", verified: true },
        }) });
        expect(invoke.mock.calls.filter(([cmd]) => cmd === "probe_chat_model")).toHaveLength(0);
    });
    it("躺平完成后更换主力或辅助模型均直接保存", async () => {
        setup();
        const provider = { id: 44, name: "DeepSeek", api_type: "deepseek", is_enabled: true };
        mockInvokeHandler("get_llm_providers", () => [provider]);
        mockInvokeHandler("get_filtered_models_for_select", args => args?.assistantType === 4 ? [] :
            ["main", "small"].map((code, i) => ({ llm_provider_id: 44, id: i + 1, name: code, code })));
        render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: /躺平/ }));
        fireEvent.change(await screen.findByLabelText("API Key"), { target: { value: "synthetic" } });
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        await selectRecommendedAndApply();
        fireEvent.click(await screen.findByRole("button", { name: "换一个" }));
        fireEvent.change(await screen.findByLabelText("轻量模型"), { target: { value: "main%%44" } });
        fireEvent.click(screen.getByRole("button", { name: "应用" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("update_onboarding_roles", expect.objectContaining({
            light: { providerId: 44, code: "main", verified: true },
        })));
        fireEvent.click(await screen.findByRole("button", { name: "换一个" }));
        fireEvent.change(await screen.findByLabelText("新对话默认使用"), { target: { value: "small%%44" } });
        fireEvent.click(screen.getByRole("button", { name: "应用" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("apply_onboarding_config", { config: expect.objectContaining({
            chat: { providerId: 44, code: "small", verified: true },
            butler: { providerId: 44, code: "small", verified: true },
        }) }));
        expect(invoke.mock.calls.filter(([cmd]) => cmd === "probe_chat_model")).toHaveLength(0);
    });
    it("should resume the saved provider at its balance check without another key or provider", async () => {
        const backend = setup();
        backend.balance(0);
        const first = render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: /躺平/ }));
        fireEvent.change(await screen.findByLabelText("API Key"), { target: { value: "synthetic" } });
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        fireEvent.click(await screen.findByRole("button", { name: "保存配置，稍后充值" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("save_onboarding_recharge", { providerId: 44 }));
        first.unmount();
        backend.balance(5);
        render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: "我已充值，重新检查" }));
        await selectRecommendedAndApply();
        await screen.findByRole("heading", { name: "好了，可以开始了" });
        expect(invoke.mock.calls.filter(([c]) => c === "add_llm_provider")).toHaveLength(1);
    });
    it.each([
        ["Claude Code", "acp"],
        ["Codex", "acp"],
        ["Pi", "__agent_default__"],
    ])("should use the native model code for %s", async (name, code) => {
        setup();
        render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: /躺平/ }));
        fireEvent.click(await screen.findByRole("button", { name: name + " · 已安装" }));
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        fireEvent.change(await screen.findByLabelText("轻量模型"), { target: { value: "agent-small%%44" } });
        expect(screen.getByText(/额外消耗大量 token/)).toBeVisible();
        expect(screen.queryByLabelText("审核模型")).toBeNull();
        fireEvent.click(screen.getByRole("button", { name: "应用" }));
        await screen.findByRole("heading", { name: "好了，可以开始了" });
        expect(invoke).toHaveBeenCalledWith("apply_onboarding_config", {
            config: expect.objectContaining({
                chat: { providerId: 44, code, verified: true },
                butler: { providerId: 44, code, verified: true },
                light: { providerId: 44, code: "agent-small", verified: true },
            }),
        });
        expect(invoke.mock.calls.some(([c]) => c === "probe_chat_model")).toBe(false);
        expect(invoke.mock.calls.filter(([cmd]) => cmd === "check_onboarding_agent")).toHaveLength(1);
    });
    it("should recheck the same Cursor account after browser login and then apply its native role", async () => {
        setup();
        let signedIn = false;
        mockInvokeHandler("detect_native_agent_environment", () => ({
            tool: { account: { signed_in: signedIn } },
        }));
        mockInvokeHandler("login_cursor", () => {
            signedIn = true;
        });
        render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: /躺平/ }));
        fireEvent.click(await screen.findByRole("button", { name: "Cursor · 已安装" }));
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        fireEvent.click(await screen.findByRole("button", { name: "登录 Cursor" }));
        fireEvent.click(await screen.findByRole("button", { name: "应用" }));
        await screen.findByRole("heading", { name: "好了，可以开始了" });
        expect(invoke).toHaveBeenCalledWith("login_cursor", expect.objectContaining({ providerId: 44 }));
        expect(invoke.mock.calls.filter(([c]) => c === "add_llm_provider")).toHaveLength(1);
        expect(invoke.mock.calls.filter(([c]) => c === "detect_native_agent_environment")).toHaveLength(2);
    });
    it("should keep an unconfigured agent out of onboarding until its model check succeeds", async () => {
        setup();
        mockInvokeHandler("check_onboarding_agent", () => { throw new Error("Claude Code 未登录"); });
        render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: /躺平/ }));
        fireEvent.click(await screen.findByRole("button", { name: "Claude Code · 已安装" }));
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        expect((await screen.findAllByText(/Claude Code 未登录/)).length).toBeGreaterThan(0);
        expect(screen.queryByLabelText("轻量模型")).toBeNull();
        expect(invoke.mock.calls.some(([cmd]) => cmd === "apply_onboarding_config")).toBe(false);
        mockInvokeHandler("check_onboarding_agent", () => ["actual-cli-model"]);
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        expect(await screen.findByRole("option", {name: "actual-cli-model"})).toBeInTheDocument();
        expect(invoke.mock.calls.filter(([cmd]) => cmd === "add_llm_provider")).toHaveLength(1);
    });
    it("should keep the chosen local chat assistant and independent API roles when entering expert chat", async () => {
        setup();
        const native = { id: 5, name: "Claude Code", api_type: "claude_sdk", is_enabled: true };
        const api = { id: 6, name: "Ollama", api_type: "ollama", is_enabled: true };
        mockInvokeHandler("get_llm_providers", () => [native, api]);
        mockInvokeHandler("get_filtered_models_for_select", (args) =>
            args?.assistantType === 4
                ? [{ llm_provider_id: 5, id: 0, code: "acp", name: "Claude Code" }]
                : ["quick", "quick-2"].map((code, index) => ({
                      llm_provider_id: 6,
                      id: 7 + index,
                      code,
                      name: code,
                  }))
        );
        mockInvokeHandler("get_office_preview_plugin_status", () => ({ installed: false }));
        mockInvokeHandler("check_bun_version", () => "1.3.2");
        mockInvokeHandler("check_uv_version", () => "Not Installed");
        mockInvokeHandler("get_onboarding_setup_status", () => ["skills", "office", "uv"].map(id => ({
            id, state: id === "uv" ? "skipped" : "done", message: "之前已就绪",
        })));
        let skills: true | false | "conflict" = true;
        let finishFirstSkillsCheck: ((ready: boolean) => void) | undefined;
        let skillsChecks = 0;
        mockInvokeHandler("check_onboarding_skills_ready", () => {
            if (++skillsChecks === 1) return new Promise<boolean>(resolve => { finishFirstSkillsCheck = resolve; });
            if (skills === "conflict") throw new Error("Skill beta 已有其他来源，保留现有目录");
            return skills;
        });
        render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: /专家/ }));
        fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
        fireEvent.change(await screen.findByLabelText("新对话默认使用"), { target: { value: "acp%%5" } });
        fireEvent.change(screen.getByLabelText("轻量模型"), { target: { value: "quick%%6" } });
        expect(screen.getByText(/Ask 弹窗.*未配置 API 模型/)).toBeInTheDocument();
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        fireEvent.click(await screen.findByRole("button", { name: "聊天" }));
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        await waitFor(() => expect(screen.getByRole("checkbox", { name: /组件预览环境/ })).toBeDisabled());
        const skillsBox = screen.getByRole("checkbox", { name: /AIPP Skills/ });
        fireEvent.focus(window);
        expect(skillsChecks).toBe(1);
        finishFirstSkillsCheck!(true);
        await waitFor(() => expect(skillsBox).toBeDisabled());
        expect(screen.getByRole("checkbox", { name: /Python 环境/ })).toBeEnabled();
        expect(screen.getByRole("checkbox", { name: /Office 插件/ })).toBeEnabled();
        skills = "conflict";
        fireEvent.focus(window);
        await screen.findByText(/Skill beta 已有其他来源/);
        expect(skillsBox).toBeEnabled();
        skills = false;
        fireEvent.click(screen.getByRole("button", { name: "重新检测" }));
        await waitFor(() => expect(screen.queryByText(/Skill beta 已有其他来源/)).not.toBeInTheDocument());
        expect(skillsBox).toBeEnabled();
        expect(invoke).not.toHaveBeenCalledWith("apply_onboarding_config", expect.anything());
        fireEvent.click(screen.getByRole("checkbox", { name: /Office 插件/ }));
        fireEvent.click(await screen.findByRole("button", { name: "完成" }));
        await screen.findByRole("button", { name: "打开聊天" });
        expect(invoke).toHaveBeenCalledWith("apply_onboarding_config", {
            config: expect.objectContaining({
                home: "chat_ui",
                extras: ["skills", "bun", "uv"],
                chat: { providerId: 5, code: "acp", verified: true },
                light: { providerId: 6, code: "quick", verified: true },
                review: { providerId: 6, code: "quick", verified: true },
            }),
        });
        fireEvent.click(screen.getByRole("button", { name: "换一个" }));
        fireEvent.change(await screen.findByLabelText("轻量模型"), { target: { value: "quick-2%%6" } });
        fireEvent.click(screen.getByRole("button", { name: "应用" }));
        await screen.findByRole("button", { name: "打开聊天" });
        expect(invoke).toHaveBeenCalledWith("update_onboarding_roles", {
            light: { providerId: 6, code: "quick-2", verified: true },
            review: { providerId: 6, code: "quick-2", verified: true },
            notes: {light:{reason:"手动选择"},review:{reason:"手动选择"}},
        });
        expect(invoke.mock.calls.filter(([c]) => c === "apply_onboarding_config")).toHaveLength(1);
        expect(invoke.mock.calls.filter(([c]) => c === "probe_chat_model")).toHaveLength(2);
    });
    it("should show the first sync failure and check sources after a successful retry", async () => {
        setup();
        const firstSync = { mode: "remote", syncing: false, last_sync_at: null, last_error: "首次同步连接被拒绝" };
        mockInvokeHandler("get_sync_status", () => firstSync);
        mockInvokeHandler("check_onboarding_sync", () => ({ completed: false, reason: "请选择模型来源" }));
        render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: /登录同步/ }));
        expect(await screen.findByText(firstSync.last_error)).toBeVisible();
        expect(invoke.mock.calls.some(([name]) => name === "check_onboarding_sync")).toBe(false);
        await act(() => emit("sync_status_changed", { ...firstSync, syncing: true }));
        expect(await screen.findByText("正在等待同步完成……")).toBeVisible();
        await act(() => emit("sync_status_changed", { ...firstSync, last_error: null, last_sync_at: "2026-11-25" }));
        expect(await screen.findByText("请选择模型来源")).toBeVisible();
    });
    it.each([false, true])("should only lock the stored workspace on a completed rerun (locked=%s)", async (locked) => {
        const backend = setup();
        backend.configure({
            state: locked ? "completed" : "in_progress", step: "path_chosen", path: "expert", rerun: locked,
            workspacePath: "C:/existing", workspaceLocked: locked,
        });
        mockInvokeHandler("get_all_feature_config", () => [
            { feature_code: "experimental", key: "butler_main_workspace_path", value: "C:/existing" },
        ]);
        mockInvokeHandler("get_llm_providers", () => [{ id: 5, name: "Ollama", api_type: "ollama", is_enabled: true }]);
        mockInvokeHandler("get_filtered_models_for_select", args => args?.assistantType === 0
            ? [{ llm_provider_id: 5, id: 1, code: "main", name: "main" }] : []);
        render(<OnboardingWindow />);
        const next = await screen.findByRole("button", { name: "下一步" });
        await waitFor(() => expect(next).toBeEnabled());
        fireEvent.click(next);
        fireEvent.change(await screen.findByLabelText("新对话默认使用"), { target: { value: "main%%5" } });
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        fireEvent.click(await screen.findByRole("button", { name: "管家" }));
        const path = screen.getByPlaceholderText("选择或输入主工作区目录路径");
        expect(path).toHaveValue("C:/existing");
        if (locked) expect(path).toBeDisabled();
        else {
            expect(path).toBeEnabled();
            fireEvent.change(path, { target: { value: "C:/chosen" } });
            expect(path).toHaveValue("C:/chosen");
        }
    });
    it("should rank all three roles, retry a missing model, and keep recommendation reasons out of the UI", async () => {
        const backend = setup();
        const codes = ["retired", "main", "mini", "review", "manual"];
        mockInvokeHandler("get_onboarding_preset", () => ({
            ...preset,
            recommendation_catalog: {providers:{fixture:{endpoints:["https://fixture.test"],
                models:Object.fromEntries(codes.map(code=>[code,{model:"lab/"+code,input:1,output:1}]))}},
                benchmarks:Object.fromEntries(codes.map(code=>["lab/"+code,[
                    {group:"reasoning",test:"general",score:code==="retired"?99:code==="main"?90:0},
                    {group:"context",test:"summary",score:code==="mini"?95:0},
                    {group:"safety",test:"review",score:code==="review"?91:0},
                ]]))},
        }));
        mockInvokeHandler("get_llm_providers", () => [{ id: 6, api_type: "ollama", name: "Ollama", is_enabled: true }]);
        mockInvokeHandler("get_filtered_models_for_select", args => args?.assistantType === 0
            ? codes.map((code, index) => ({ id: index + 1, llm_provider_id: 6, code, name: code })) : []);
        mockInvokeHandler("probe_chat_model", args =>
            args?.modelCode === "retired" ? { ok: false, kind: "model_not_found", message: "404" } : { ok: true });
        mockInvokeHandler("apply_onboarding_config", args => {
            const config = args!.config as Config;
            const applied: OnboardingState = {
                state: "completed", path: "expert", step: "applied", activeLight: "mini%%6", activeReview: "review%%6",
                draft: { providers: [], config }, workspacePath: "C:/AIPP", canOpen: true, workspaceLocked: false, rerun: false,
            };
            backend.configure(applied);
            return applied;
        });
        render(<OnboardingWindow />);
        fireEvent.click(await screen.findByRole("button", { name: /专家/ }));
        fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
        const callsBeforePick = invoke.mock.calls.length;
        fireEvent.click(await screen.findByRole("button", { name: "全部帮我选" }));
        expect(invoke.mock.calls).toHaveLength(callsBeforePick);
        expect(screen.getByLabelText("新对话默认使用")).toHaveValue("retired%%6");
        expect(invoke.mock.calls.filter(([cmd]) => cmd === "probe_chat_model")).toHaveLength(0);
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
        fireEvent.click(await screen.findByRole("button", { name: "完成" }));
        await screen.findByRole("button", { name: "打开聊天" });
        expect(screen.queryByText(/选择依据/)).toBeNull();
        expect(invoke).toHaveBeenCalledWith("apply_onboarding_config", { config: expect.objectContaining({
            chat: {providerId:6,code:"main",verified:true}, light:{providerId:6,code:"mini",verified:true}, review:{providerId:6,code:"review",verified:true},
        }) });
        fireEvent.click(screen.getByRole("button", { name: "换一个" }));
        expect(screen.queryByText(/选择依据/)).toBeNull();
        fireEvent.change(screen.getByLabelText("轻量模型"), { target: { value: "manual%%6" } });
        expect(screen.queryByText(/选择依据/)).toBeNull();
        expect(screen.queryByText("选择依据 · 推荐表第 3 名")).toBeNull();
        fireEvent.click(screen.getByRole("button", { name: "应用" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("update_onboarding_roles", expect.objectContaining({
            light: { providerId: 6, code: "manual", verified: true }, notes: { light: { reason: "手动选择" } },
        })));
    });
    it("should apply a durable source_ready draft directly after restart", async () => {
        const backend = setup();
        const chat = { providerId: 17, code: "model", verified: true };
        backend.configure({
            step: "source_ready",
            path: "expert",
            state: "in_progress",
            draft: { providers: [], config: { chat, home: "chat_ui", extras: [] } },
        });
        render(<OnboardingWindow />);
        await screen.findByRole("button", { name: "打开聊天" });
        expect(invoke.mock.calls.some(([c]) => c === "probe_chat_model")).toBe(false);
    });
});
