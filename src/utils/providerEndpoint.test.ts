import { describe, expect, it } from "vitest";
import { getProviderEndpointStatus } from "./providerEndpoint";

describe("provider endpoint", () => {
    it("should distinguish an unset endpoint from an invalid one when input changes", () => {
        expect(getProviderEndpointStatus("  ").kind).toBe("unset");
        expect(getProviderEndpointStatus("bad").kind).toBe("invalid");
        expect(getProviderEndpointStatus("bad").message).not.toContain("api.openai.com");
    });
    it.each(["http:///host", "https:host", "http://", "ftp://localhost", "http://local\nhost", "http://localhost\\path", "http://user:secret@localhost", "http://localhost/v1?", "http://localhost/v1#"])(
        "should reject a malformed base URL when input is %s", (endpoint) => {
            const status = getProviderEndpointStatus(endpoint);
            expect(status.kind).toBe("invalid");
            expect(status.message).not.toContain("secret");
        },
    );
    it("should show the normalized destination when input is a valid custom endpoint", () => {
        expect(getProviderEndpointStatus("  http://127.0.0.1:8080/v1  ")).toEqual({
            kind: "custom", message: "请求基址：http://127.0.0.1:8080/v1/",
        });
    });
});
