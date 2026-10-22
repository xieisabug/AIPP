import { describe, expect, it } from "vitest";

import { splitClosedMarkdownBlocks } from "./streamingMarkdownBlocks";

describe("splitClosedMarkdownBlocks", () => {
    it("keeps an unfinished paragraph in the open tail", () => {
        expect(splitClosedMarkdownBlocks("hello")).toEqual({
            closed: [],
            open: "hello",
        });
    });

    it("moves a paragraph into closed blocks once the next top-level block starts", () => {
        expect(splitClosedMarkdownBlocks("hello\n\nworld")).toEqual({
            closed: ["hello"],
            open: "world",
        });
    });

    it("keeps a paragraph open while only a blank line has arrived", () => {
        expect(splitClosedMarkdownBlocks("hello\n\n")).toEqual({
            closed: [],
            open: "hello\n\n",
        });
    });

    it("keeps an open fence together until it closes", () => {
        const content = "intro\n\n```ts\nconst value = 1\n\nconst other = 2";
        expect(splitClosedMarkdownBlocks(content)).toEqual({
            closed: ["intro"],
            open: "```ts\nconst value = 1\n\nconst other = 2",
        });
    });

    it("closes a fence when the next top-level block starts", () => {
        const content = "```ts\nconst value = 1\n```\n\nnext";
        expect(splitClosedMarkdownBlocks(content)).toEqual({
            closed: ["```ts\nconst value = 1\n```"],
            open: "next",
        });
    });

    it("does not split inside an indented fence of a list item", () => {
        const content = "1. step\n\n   ```bash\n   echo a\n\n   echo b\n   ```\n\n2. next step";
        expect(splitClosedMarkdownBlocks(content)).toEqual({
            closed: [],
            open: content,
        });
    });

    it("keeps loose list items and indented continuations in one block", () => {
        const content = "- a\n\n    continued\n\n- b\n\nafter";
        expect(splitClosedMarkdownBlocks(content)).toEqual({
            closed: ["- a\n\n    continued\n\n- b"],
            open: "after",
        });
    });

    it("does not split inside a display math block", () => {
        const content = "$$\na = b\n\nc = d\n$$\n\ntext";
        expect(splitClosedMarkdownBlocks(content)).toEqual({
            closed: ["$$\na = b\n\nc = d\n$$"],
            open: "text",
        });
    });

    it("gives up when html blocks, link definitions or footnotes are present", () => {
        expect(splitClosedMarkdownBlocks("a\n\n<details>\n\nb\n\n</details>")).toBeNull();
        expect(splitClosedMarkdownBlocks("see [x]\n\n[x]: https://example.com")).toBeNull();
        expect(splitClosedMarkdownBlocks("note[^1]\n\n[^1]: detail")).toBeNull();
    });

    it("allows html-looking lines inside code fences", () => {
        const content = "```html\n<div>\n\n</div>\n```\n\nafter";
        expect(splitClosedMarkdownBlocks(content)).toEqual({
            closed: ["```html\n<div>\n\n</div>\n```"],
            open: "after",
        });
    });
});
