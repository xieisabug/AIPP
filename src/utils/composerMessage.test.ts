import { describe, expect, it } from "vitest";
import { AttachmentType } from "@/data/Conversation";
import { newComposerMessage } from "./composerMessage";

describe("newComposerMessage attachments", () => {
    it("should leave attachment_list empty when nothing is attached", () => {
        expect(newComposerMessage(3, "hi").attachment_list).toBeUndefined();
        expect(newComposerMessage(3, "hi", []).attachment_list).toBeUndefined();
    });

    it("should skip an image without stored content instead of using its path as the image source", () => {
        const row = newComposerMessage(3, "看图", [
            { id: 1, name: "a.png", path: "C:/files/a.png", type: AttachmentType.Image },
            { id: 2, name: "notes.md", path: "C:/files/notes.md", type: AttachmentType.Text },
        ]);
        expect(row.attachment_list).toEqual([
            { id: 2, attachment_type: "Text", attachment_url: "C:/files/notes.md", attachment_content: undefined },
        ]);
    });

    it("should allocate optimistic IDs outside butlerTimeline synthetic ranges", () => {
        const row = newComposerMessage(3, "hi");
        expect(row.id).toBeLessThanOrEqual(-7_000_000_000_000);
    });
});
