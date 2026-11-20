import { describe, expect, it } from "vitest";
import type { MCPServer } from "@/data/MCP";
import { buildDeepSeekSearchEnv, deepSeekSearchEndpoint, deepSeekSearchStatus, findSearchServer, hideKey, parseEnvText } from "./deepseekSuite";

function searchServer(env: string, extra: Partial<MCPServer> = {}): MCPServer {
    return {
        id: 1, name: "搜索", description: null, transport_type: "stdio", command: "aipp:search", environment_variables: env,
        url: null, timeout: 60000, is_long_running: false, is_enabled: true, is_builtin: true, is_deletable: true, created_time: "",
        ...extra,
    };
}

describe("deepseekSuite", () => {
    it("parses env text like the backend search handler", () => {
        expect(parseEnvText("# note\n A = 1 \n\nB=x=y\nbad\n=empty")).toEqual([["A", "1"], ["B", "x=y"]]);
    });

    it("uses the default endpoint for the official API and the same origin for a relay", () => {
        expect(deepSeekSearchEndpoint("")).toBe("");
        expect(deepSeekSearchEndpoint("https://api.deepseek.com/")).toBe("");
        expect(deepSeekSearchEndpoint("https://api.deepseek.com/v1")).toBe("");
        expect(deepSeekSearchEndpoint("https://relay.example/deepseek/v1")).toBe("https://relay.example/v1/tools/search");
        expect(deepSeekSearchEndpoint("not a url")).toBe("");
    });

    it("switches HTTP search and fetch to DeepSeek and keeps unrelated settings", () => {
        const env = buildDeepSeekSearchEnv(
            "SEARCH_ENGINE=bing\nHTTP_SEARCH_PROVIDER=tavily\nHTTP_SEARCH_ENDPOINT=https://api.tavily.com/search\nHTTP_SEARCH_API_KEY=tvly\nFETCH_PROVIDER=local",
            "vault://3f2a9c1e-0b7d-4e5f-9a12-6c8d0e1f2a3b",
            "https://api.deepseek.com/",
        );
        expect(env.split("\n")).toEqual([
            "SEARCH_ENGINE=bing",
            "HTTP_SEARCH_PROVIDER=deepseek",
            "HTTP_SEARCH_API_KEY=vault://3f2a9c1e-0b7d-4e5f-9a12-6c8d0e1f2a3b",
            "FETCH_PROVIDER=deepseek",
        ]);
        expect(buildDeepSeekSearchEnv(null, "k", "https://relay.example/v1")).toContain("HTTP_SEARCH_ENDPOINT=https://relay.example/v1/tools/search");
    });

    it("reports how the provider key relates to the built-in search", () => {
        const key = "vault://3f2a9c1e-0b7d-4e5f-9a12-6c8d0e1f2a3b";
        expect(deepSeekSearchStatus([], key)).toEqual({ kind: "none" });
        // 本地搜索（含未设置与旧值 disabled）是一种在用的搜索服务，可以改用 DeepSeek
        for (const env of ["", "HTTP_SEARCH_PROVIDER=disabled", "HTTP_SEARCH_PROVIDER=local"]) {
            expect(deepSeekSearchStatus([searchServer(env)], key)).toMatchObject({ kind: "other_provider", label: "本地搜索" });
        }
        expect(deepSeekSearchStatus([searchServer("HTTP_SEARCH_PROVIDER=brave")], key)).toMatchObject({ kind: "other_provider", label: "Brave Search" });
        expect(deepSeekSearchStatus([searchServer("HTTP_SEARCH_PROVIDER=deepseek\nHTTP_SEARCH_API_KEY=other")], key).kind).toBe("other_key");
        expect(deepSeekSearchStatus([searchServer(`HTTP_SEARCH_PROVIDER=deepseek\nHTTP_SEARCH_API_KEY=${key}\nFETCH_PROVIDER=deepseek`)], key))
            .toMatchObject({ kind: "connected", enabled: true, fetch: true });
        expect(deepSeekSearchStatus([searchServer(`HTTP_SEARCH_PROVIDER=deepseek\nHTTP_SEARCH_API_KEY=${key}`, { is_enabled: false })], key))
            .toMatchObject({ kind: "connected", enabled: false, fetch: false });
    });

    it("prefers the enabled built-in search server and ignores user servers", () => {
        const disabled = searchServer("", { id: 1, is_enabled: false });
        const enabled = searchServer("", { id: 2 });
        const custom = searchServer("", { id: 3, is_builtin: false });
        expect(findSearchServer([custom, disabled, enabled])?.id).toBe(2);
        expect(findSearchServer([custom])).toBeUndefined();
    });

    it("hides a plain key in error text but not a vault reference", () => {
        expect(hideKey("failed: sk-123 rejected", " sk-123 ")).toBe("failed: [已隐藏] rejected");
        const ref = "vault://3f2a9c1e-0b7d-4e5f-9a12-6c8d0e1f2a3b";
        expect(hideKey(`bad ${ref}`, ref)).toBe(`bad ${ref}`);
    });
});
