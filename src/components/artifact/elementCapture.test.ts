import { describe, expect, it } from "vitest";
import { elementCapture } from "./elementCapture";

describe("elementCapture", () => {
    const versions = [{ version: 3 }, { version: 2 }];

    it("should capture conversation artifacts on their latest version", () => {
        expect(elementCapture(7, { artifactKey: "ui/card", currentVersion: 3, versions })).toBeTypeOf("function");
        expect(elementCapture(7, { artifactKey: "ui/card", versions: [] })).toBeTypeOf("function");
    });

    it("should not capture old versions or artifacts outside a conversation", () => {
        expect(elementCapture(7, { artifactKey: "ui/card", currentVersion: 2, versions })).toBeUndefined();
        expect(elementCapture(undefined, { artifactKey: "ui/card", versions })).toBeUndefined();
        expect(elementCapture(7, { artifactKey: null, versions })).toBeUndefined();
    });
});
