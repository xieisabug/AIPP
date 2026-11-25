import { describe, expect, it } from "vitest";
import { parseEnvVars, serializeEnvVars } from "./EnvVarEditor";

describe("EnvVarEditor serialization", () => {
    it("parses both dotenv lines and JSON objects", () => {
        expect(parseEnvVars("FOO=bar\n# comment\nBAZ=\"hello world\"")).toEqual([
            { key: "FOO", value: "bar" },
            { key: "BAZ", value: "hello world" },
        ]);

        expect(parseEnvVars('{"OPENAI_BASE_URL":"https://api.example.com","DEBUG":true}')).toEqual([
            { key: "OPENAI_BASE_URL", value: "https://api.example.com" },
            { key: "DEBUG", value: "true" },
        ]);
    });

    it("serializes cleanly to dotenv or JSON format", () => {
        const rows = [
            { key: "A", value: "1" },
            { key: "  ", value: "ignored" },
            { key: "B", value: "two" },
        ];
        expect(serializeEnvVars(rows, "dotenv")).toBe("A=1\nB=two");
        expect(JSON.parse(serializeEnvVars(rows, "json"))).toEqual({ A: "1", B: "two" });
        expect(serializeEnvVars([], "json")).toBe("");
    });
});
