import { describe, expect, it } from "vitest";
import {
    isAippBuiltinCommand,
    isMobileExecutableAssistantType,
    isMobileExecutableMcpServer,
    isMobileUnsupportedError,
    MOBILE_UNSUPPORTED_ERROR_PREFIX,
} from "./mobileUnsupported";

describe("mobileUnsupported helpers", () => {
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
});
