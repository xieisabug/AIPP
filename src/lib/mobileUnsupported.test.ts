import { describe, expect, it } from "vitest";
import {
    getMcpServerDisplayLabel,
    isAippBuiltinCommand,
    isMobileExecutableAssistantType,
    isMobileExecutableMcpServer,
    isMobileUnsupportedError,
    MOBILE_UNSUPPORTED_ERROR_PREFIX,
} from "./mobileUnsupported";

describe("mobileUnsupported helpers", () => {
    it("should reject forged builtin metadata when the command needs an external process", () => {
        expect(isMobileExecutableMcpServer({ transport_type: "stdio", command: "npx server", is_builtin: true })).toBe(false);
        expect(isMobileExecutableMcpServer({ transport_type: "stdio", command: "aipp:unknown", is_builtin: true })).toBe(false);
        expect(isMobileExecutableMcpServer({ transport_type: "builtin", command: "aipp:search" })).toBe(true);
        expect(isMobileExecutableMcpServer({ transport_type: "stdio", command: "aipp:search --execute" })).toBe(false);
    });
    it("recognizes aipp:* builtin commands even when registered as stdio", () => {
        expect(isAippBuiltinCommand("aipp:search")).toBe(true);
        expect(isAippBuiltinCommand(" aipp:operation ")).toBe(true);
        expect(isAippBuiltinCommand("npx some-server")).toBe(false);
        expect(isAippBuiltinCommand(null)).toBe(false);
    });

    it("allows HTTP and aipp:* on mobile, blocks external stdio", () => {
        expect(
            isMobileExecutableMcpServer({
                transport_type: "http",
                command: null,
            }),
        ).toBe(true);
        expect(
            isMobileExecutableMcpServer({
                transport_type: "stdio",
                command: "aipp:search",
            }),
        ).toBe(true);
        expect(
            isMobileExecutableMcpServer({
                transport_type: "stdio",
                command: "npx @modelcontextprotocol/server-filesystem /tmp",
            }),
        ).toBe(false);
        expect(
            isMobileExecutableMcpServer({
                transport_type: "stdio",
                is_builtin: true,
                command: "aipp:operation",
            }),
        ).toBe(true);
    });

    it("only treats normal API assistants as mobile-executable", () => {
        expect(isMobileExecutableAssistantType(0)).toBe(true);
        expect(isMobileExecutableAssistantType(undefined)).toBe(true);
        expect(isMobileExecutableAssistantType(4)).toBe(false);
        expect(isMobileExecutableAssistantType(2)).toBe(false);
    });

    it("detects MOBILE_UNSUPPORTED errors including AppError wrappers", () => {
        expect(isMobileUnsupportedError(`${MOBILE_UNSUPPORTED_ERROR_PREFIX}: x`)).toBe(true);
        expect(
            isMobileUnsupportedError(`未知错误: ${MOBILE_UNSUPPORTED_ERROR_PREFIX}: 外部 stdio MCP`),
        ).toBe(true);
        expect(isMobileUnsupportedError("network timeout")).toBe(false);
    });

    it("marks external stdio MCP in the mobile dropdown label only", () => {
        const stdio = { name: "filesystem", transport_type: "stdio", command: "npx -y server-filesystem /tmp" };
        const builtin = { name: "搜索", transport_type: "stdio", command: "aipp:search", is_builtin: true };
        const http = { name: "remote", transport_type: "http", command: null };
        expect(getMcpServerDisplayLabel(stdio, true)).toBe("filesystem（此设备不可执行）");
        expect(getMcpServerDisplayLabel(builtin, true)).toBe("搜索");
        expect(getMcpServerDisplayLabel(http, true)).toBe("remote");
        // 桌面平台不追加标记
        expect(getMcpServerDisplayLabel(stdio, false)).toBe("filesystem");
    });
});

describe("isMobileUnexecutableBuiltinTool", () => {
    it("marks only browser/shell tools of aipp:* servers", async () => {
        const { isMobileUnexecutableBuiltinTool } = await import("./mobileUnsupported")
        expect(isMobileUnexecutableBuiltinTool("aipp:search", "search_web")).toBe(true)
        expect(isMobileUnexecutableBuiltinTool("aipp:search", "fetch_url")).toBe(false)
        expect(isMobileUnexecutableBuiltinTool("aipp:search", "search_http")).toBe(false)
        expect(isMobileUnexecutableBuiltinTool("aipp:operation", "execute_bash")).toBe(true)
        expect(isMobileUnexecutableBuiltinTool("aipp:operation", "read_file")).toBe(false)
        expect(isMobileUnexecutableBuiltinTool("npx foo", "search_web")).toBe(false)
        expect(isMobileUnexecutableBuiltinTool(null, "search_web")).toBe(false)
    })
})


describe("visibleSearchTools", () => {
    it("should hide desktop browser search only on mobile without changing saved tools", async () => {
        const { visibleSearchTools } = await import("./mobileUnsupported");
        const tools = ["search_web", "search_http", "fetch_url"].map(name => ({ name, is_enabled: true }));
        expect(visibleSearchTools(tools, "aipp:search", true).map(t => t.name)).toEqual(["search_http", "fetch_url"]);
        expect(visibleSearchTools(tools, "aipp:search", false)).toBe(tools);
        expect(visibleSearchTools(tools, "external", true)).toBe(tools);
        expect(tools).toHaveLength(3);
        expect(visibleSearchTools([{ tool_name: "search_web" }], "aipp:search", true)).toEqual([]);
    });
});
