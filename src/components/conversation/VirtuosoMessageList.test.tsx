import { describe, expect, it } from "vitest";

import { getVirtuosoRowMinHeight } from "./VirtuosoMessageList";

describe("VirtuosoMessageList row height reservation", () => {
    it("keeps the first history row's estimated min height", () => {
        expect(
            getVirtuosoRowMinHeight(0, { estimatedHeight: 240 }),
        ).toBe(240);
    });

    it("does not force estimated min height on later history rows", () => {
        expect(
            getVirtuosoRowMinHeight(1, { estimatedHeight: 240 }),
        ).toBeUndefined();
    });
});
