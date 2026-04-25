import { describe, expect, it } from "vitest";

import {
    buildButlerWorkspaceConfig,
    BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION,
    serializeButlerWorkspaceConfig,
} from "./butlerWorkspaceConfig";

describe("butlerWorkspaceConfig", () => {
    it("should derive the main workspace from legacy trusted workspace config", () => {
        const config = buildButlerWorkspaceConfig({
            trustedWorkspacesRaw: JSON.stringify([
                { path: "E:\\main", description: "" },
                { path: "E:\\extra", description: "extra" },
            ]),
        });

        expect(config.mainWorkspace).toEqual({
            path: "E:\\main",
            description: BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION,
        });
        expect(config.trustedWorkspaces).toEqual([
            { path: "E:\\extra", description: "extra" },
        ]);
    });

    it("should keep the main workspace separate when serializing", () => {
        const serialized = serializeButlerWorkspaceConfig({
            mainWorkspacePath: "E:\\main",
            mainWorkspaceDescription: "",
            trustedWorkspaces: [
                { path: "E:\\main", description: "duplicate" },
                { path: "E:\\extra", description: "extra" },
            ],
        });

        expect(serialized.mainWorkspacePath).toBe("E:\\main");
        expect(serialized.mainWorkspaceDescription).toBe(
            BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION
        );
        expect(serialized.trustedWorkspacesRaw).toBe(
            JSON.stringify([{ path: "E:\\extra", description: "extra" }])
        );
    });
});
