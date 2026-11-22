import { describe, it, expect, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { useForm } from "react-hook-form";
import { mockInvokeHandler, clearAllMockHandlers } from "@/__tests__/mocks/tauri";
import { findProviderApiType } from "./providerApiTypes";
import { isDecisionRequestMode, supportsRequestModeToggle } from "./llmModelTypes";
import ModelRequestModeToggle from "./ModelRequestModeToggle";
import { SummaryConfigForm } from "./feature/forms/SummaryConfigForm";
import { ExperimentalConfigForm } from "./feature/forms/ExperimentalConfigForm";
import {
    EXPERIMENTAL_CONFIG_DEFAULT_VALUES,
    type ExperimentalConfigFormState,
} from "./feature/forms/experimentalConfigShared";

describe("Decision model UI configuration (#340)", () => {
    beforeEach(() => {
        clearAllMockHandlers();
    });

    it("registers gemini and typesafe_jev providers and decision request modes", () => {
        expect(findProviderApiType("typesafe_jev")).toMatchObject({
            value: "typesafe_jev",
            kind: "http",
            defaultEndpoint: "https://api.typesafe.ai/v1/",
        });
        expect(findProviderApiType("gemini")).toMatchObject({
            value: "gemini",
            kind: "http",
            defaultEndpoint: "https://generativelanguage.googleapis.com/v1beta/",
        });

        expect(isDecisionRequestMode("jev_systemone")).toBe(true);
        expect(isDecisionRequestMode("openai_decisions")).toBe(true);
        expect(isDecisionRequestMode("gemini_decisions")).toBe(true);
        expect(isDecisionRequestMode("chat_completions")).toBe(false);
        expect(isDecisionRequestMode("responses")).toBe(false);

        expect(supportsRequestModeToggle("openai_api")).toBe(true);
        expect(supportsRequestModeToggle("gemini")).toBe(true);
        expect(supportsRequestModeToggle("typesafe_jev")).toBe(true);
    });

    it("displays decision short labels in ModelRequestModeToggle", () => {
        const { rerender } = render(
            <ModelRequestModeToggle
                requestMode="jev_systemone"
                availableModes={["jev_systemone"]}
                onToggle={() => undefined}
            />,
        );
        expect(screen.getByText("Jev")).toBeInTheDocument();

        rerender(
            <ModelRequestModeToggle
                requestMode="openai_decisions"
                availableModes={["chat_completions", "responses", "openai_decisions"]}
                onToggle={() => undefined}
            />,
        );
        expect(screen.getByText("Decisions")).toBeInTheDocument();

        rerender(
            <ModelRequestModeToggle
                requestMode="gemini_decisions"
                availableModes={["google_gemini", "gemini_decisions"]}
                onToggle={() => undefined}
            />,
        );
        expect(screen.getByText("Gemini Dec.")).toBeInTheDocument();
    });

    it("should offer OpenAI protocols when a saved protocol needs to be reselected", async () => {
        let selected = "";
        render(<ModelRequestModeToggle
            requestMode="provider_mode_mismatch"
            availableModes={["chat_completions", "responses", "openai_decisions"]}
            onToggle={mode => { selected = mode; }}
        />);
        const trigger = screen.getByRole("button", { name: "请求协议：需重选协议" });
        fireEvent.keyDown(trigger, { key: "ArrowDown" });
        const items = await screen.findAllByRole("menuitemradio");
        expect(items).toHaveLength(3);
        expect(screen.queryByRole("menuitemradio", { name: /Jev|Gemini/ })).toBeNull();
        fireEvent.click(screen.getByRole("menuitemradio", { name: /OpenAI Decisions/ }));
        expect(selected).toBe("openai_decisions");
    });

    it("shows decision threshold & probe button instead of JSON Schema mode when auto_review_model is a decision model", async () => {
        const calls: Array<{ cmd: string; args: unknown }> = [];
        mockInvokeHandler("get_models_for_select", (args) => {
            calls.push({ cmd: "get_models_for_select", args });
            return [
                {
                    id: 1,
                    name: "TypeSafe / Jev Latest",
                    code: "jev-latest",
                    llm_provider_id: 9,
                    request_mode: "jev_systemone",
                    is_decision: true,
                },
            ];
        });
        mockInvokeHandler("test_decision_model", (args) => {
            calls.push({ cmd: "test_decision_model", args });
            return {
                ok: true,
                protocol: "jev_systemone",
                protocol_label: "Jev System One (/v1/systemone)",
                endpoint_url: "https://api.typesafe.ai/v1/systemone",
                latency_ms: 42,
                probability: 0.99,
                input_tokens: 18,
                message: "连接成功（Jev System One (/v1/systemone)，42ms，探针概率 99.0%）",
            };
        });

        function Harness() {
            const form = useForm({
                defaultValues: {
                    assistant_ai_enabled: true,
                    title_summary_enabled: false,
                    title_model: "",
                    title_summary_length: "100",
                    title_prompt: "",
                    form_autofill_enabled: false,
                    form_autofill_model: "",
                    auto_review_model: "jev-latest%%9",
                    auto_review_output_mode: "auto",
                    auto_review_decision_threshold: "0.90",
                    context_compaction_enabled: false,
                    context_max_input_tokens: "128000",
                    context_compaction_threshold: "0.80",
                    context_tail_ratio: "0.30",
                },
            });
            return <SummaryConfigForm form={form} onSave={async () => undefined} />;
        }

        render(<Harness />);

        expect(await screen.findByText("决策放行安全阈值")).toBeInTheDocument();
        expect(screen.queryByText("审核输出方式")).toBeNull();
        expect(calls.some((c) => c.cmd === "get_models_for_select" && (c.args as { includeDecision?: boolean })?.includeDecision === true)).toBe(true);

        fireEvent.click(screen.getByRole("button", { name: "测试决策接口" }));
        await waitFor(() => {
            expect(calls).toContainEqual({
                cmd: "test_decision_model",
                args: { providerId: 9, modelCode: "jev-latest" },
            });
        });
        expect(await screen.findByText(/连接成功.*99\.0%/)).toBeInTheDocument();
    });

    it("renders dedicated group dispatch model selector with probe button in ExperimentalConfigForm", async () => {
        mockInvokeHandler("get_models_for_select", (args) => {
            if ((args as { includeDecision?: boolean })?.includeDecision) {
                return [
                    {
                        id: 2,
                        name: "OpenAI / GPT-6 Luna",
                        code: "gpt-6-luna",
                        llm_provider_id: 1,
                        request_mode: "openai_decisions",
                        is_decision: true,
                    },
                ];
            }
            return [
                { id: 1, name: "OpenAI / GPT-4o", code: "gpt-4o", llm_provider_id: 1, request_mode: "chat_completions", is_decision: false },
            ];
        });
        mockInvokeHandler("get_experimental_summary_task_status", () => ({
            mcp_running: false,
            assistant_running: false,
            conversation_running: false,
            conversation_running_count: 0,
        }));
        mockInvokeHandler("get_butler_feishu_status", () => null);

        function Harness() {
            const form = useForm<ExperimentalConfigFormState>({
                defaultValues: {
                    ...EXPERIMENTAL_CONFIG_DEFAULT_VALUES,
                    butler_experiment_enabled: "true",
                    butler_group_dispatch_model_id: "gpt-6-luna%%1",
                },
            });
            return <ExperimentalConfigForm form={form} onSave={async () => undefined} scope="butler" />;
        }

        render(<Harness />);
        expect(screen.getByText("群聊发言调度模型")).toBeInTheDocument();
        expect(await screen.findByRole("button", { name: "测试决策接口" })).toBeInTheDocument();
    });
});
