import { describe, expect, it, vi } from "vitest";
import { deadline, probeSession, role, type Config } from "./onboardingService";
import { checkSelected } from "./verifySources";
import { firstVerified, rankedRoles, verifyRecommendedRoles, type RoleCandidate } from "./roleRecommendations";
import { invoke, clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
vi.mock("@tauri-apps/api/core", async () => import("@/__tests__/mocks/tauri"));
const makeCandidates = (codes: string[]) =>
    codes.map((code, index) => ({ model: role(12, code), reason: `推荐表第 ${index + 1} 名` }));
const ranked = makeCandidates(["a", "b", "c", "d", "e"]);
const missing = { kind: "model_not_found", message: "404" };
describe("onboarding role recommendation probes", () => {
    it("should compare offers across enabled sources even when no candidate has a benchmark", () => {
        const providers = ["first", "second"].map((name, index) => ({
            id: index + 1, name, api_type: "openai_api", is_enabled: true, endpoint: "https://"+name+".test",
        }));
        const choices = rankedRoles(providers, [
            { id: 1, llm_provider_id: 1, code: "tiny-mini", name: "tiny-mini" },
            { id: 2, llm_provider_id: 2, code: "ranked", name: "ranked" },
        ], {
            recommended_provider: {
                name: "DeepSeek", api_type: "deepseek", endpoint: "", price_hint: "", key_guide: [],
                models: { main: "chat", light: "light" },
            },
            recommendation_catalog: {benchmarks:{},providers:{
                first:{endpoints:["https://first.test"],models:{"tiny-mini":{input:3,output:3}}},
                second:{endpoints:["https://second.test"],models:{ranked:{input:1,output:1}}},
            }},
            model_ranking: { main: [], light: [] },
            light_keywords: ["mini"], strip_suffixes: [],
        });
        expect(choices.light.map(candidate => candidate.model.code)).toEqual(["ranked", "tiny-mini"]);
    });
    it("should stop after three replacements when all candidates are missing", async () => {
        const probe = vi.fn().mockRejectedValue(missing);
        await expect(firstVerified(ranked, probe)).rejects.toBe(missing);
        expect(probe.mock.calls.map(([model]) => model.code)).toEqual(["a", "b", "c", "d"]);
    });
    it.each(["auth", "insufficient_balance", "rate_limited", "network", "timeout"])(
        "should keep the first candidate when the probe reports %s",
        async kind => {
            const probe = vi.fn().mockRejectedValue({ kind });
            await expect(firstVerified(ranked, probe)).rejects.toEqual({ kind });
            expect(probe).toHaveBeenCalledTimes(1);
        }
    );
    it("should share the total deadline across roles and replacements", async () => {
        clearAllMockHandlers();
        invoke.mockClear();
        vi.useFakeTimers();
        vi.setSystemTime(0);
        try {
            mockInvokeHandler("probe_chat_model", args => {
                vi.setSystemTime(Date.now() + 11_000);
                return args?.modelCode === "b" ? { ok: true } : { ok: false, ...missing };
            });
            const initial: Config = { chat: role(12, ""), home: "chat_ui", extras: [] };
            const picks = { chat: ranked, light: [ranked[2], ranked[3]], review: ranked.slice(2) };
            const checked = await verifyRecommendedRoles(initial, picks, ["chat", "light", "review"], false, probeSession(deadline(), () => {}));
            expect(checked.config.chat.code).toBe("b");
            expect(checked.config.notes?.chat?.reason).toBe("推荐表第 2 名");
            expect(checked.errors).toHaveLength(1);
            expect(invoke.mock.calls.map(([, args]) => args?.modelCode)).toEqual(["a", "b", "c"]);
        } finally {
            vi.useRealTimers();
        }
    });
    it("should replace a prefilled recommendation on final verification and keep a manual choice on error", async () => {
        clearAllMockHandlers();
        invoke.mockClear();
        mockInvokeHandler("probe_chat_model", args => args?.modelCode === "a" ? { ok: false, ...missing } : { ok: true });
        const providers = [{ id: 12, api_type: "ollama", name: "Ollama", is_enabled: true }];
        const choices = { chat: ranked, light: [] as RoleCandidate[], review: [] as RoleCandidate[] };
        const config: Config = { chat: ranked[0].model, notes: { chat: { reason: ranked[0].reason } }, home: "chat_ui", extras: [] };
        const checked = await checkSelected(config, providers, () => false, () => {}, undefined, choices);
        expect(checked.config.chat.code).toBe("b");
        expect(checked.notes.chat?.reason).toBe("推荐表第 2 名");
        invoke.mockClear();
        await expect(checkSelected({ ...config, notes: { chat: { reason: "手动选择" } } }, providers, () => false, () => {}, undefined, choices)).rejects.toMatchObject(missing);
        expect(invoke.mock.calls.map(([, args]) => args?.modelCode)).toEqual(["a"]);
    });
});
