import { describe, expect, it, vi } from "vitest";
import { requestArtifactFixOpening } from "./artifactFixRequest";

vi.mock("./openConversationInChat", () => ({ openConversationInChat: vi.fn() }));

describe("requestArtifactFixOpening", () => {
    const wait = () => Promise.resolve();

    it("should deliver directly when the conversation is already open", async () => {
        const request = vi.fn().mockResolvedValue(true);
        const open = vi.fn();
        await expect(requestArtifactFixOpening(7, "改按钮", { request, open, wait })).resolves.toBe(true);
        expect(open).not.toHaveBeenCalled();
        expect(request).toHaveBeenCalledTimes(1);
    });

    it("should open the conversation and retry until the chat view picks it up", async () => {
        const request = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(false).mockResolvedValueOnce(true);
        const open = vi.fn().mockResolvedValue(undefined);
        await expect(requestArtifactFixOpening(7, "改按钮", { request, open, wait })).resolves.toBe(true);
        expect(open).toHaveBeenCalledWith(7);
        expect(request).toHaveBeenCalledTimes(3);
        expect(request).toHaveBeenLastCalledWith(7, "改按钮");
    });

    it("should give up after a few retries", async () => {
        const request = vi.fn().mockResolvedValue(false);
        const open = vi.fn().mockResolvedValue(undefined);
        await expect(requestArtifactFixOpening(7, "改按钮", { request, open, wait })).resolves.toBe(false);
        expect(request).toHaveBeenCalledTimes(5);
    });
});
