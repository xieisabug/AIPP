import { describe, it, expect } from "vitest";
import remarkCodeBlockMeta, { parseCodeBlockMeta, resolveCodeBlockMeta } from "./remarkCodeBlockMeta";

describe("remarkCodeBlockMeta", () => {
    it("parses common meta fields", () => {
        const meta = `title="App.tsx" filename='src/App.tsx' line=12 highlight="1-3,5"`;
        expect(parseCodeBlockMeta(meta)).toEqual({
            meta,
            title: "App.tsx",
            filename: "src/App.tsx",
            line: "12",
            highlight: "1-3,5",
        });
    });

    it("falls back to filename when title missing", () => {
        const meta = "filename=main.rs";
        expect(parseCodeBlockMeta(meta)).toEqual({
            meta,
            title: "main.rs",
            filename: "main.rs",
            line: undefined,
            highlight: undefined,
        });
    });

    it("resolves from data attributes first", () => {
        const props = {
            "data-title": "Hello.ts",
            "data-line": "3",
            "data-highlight": "1-2",
        };
        expect(resolveCodeBlockMeta(props)).toEqual({
            meta: undefined,
            title: "Hello.ts",
            filename: undefined,
            line: "3",
            highlight: "1-2",
        });
    });

    it("falls back to node meta when data attributes missing", () => {
        const node = { data: { meta: "title=Readme.md line=1 highlight=1" } };
        expect(resolveCodeBlockMeta({}, node)).toEqual({
            meta: "title=Readme.md line=1 highlight=1",
            title: "Readme.md",
            filename: undefined,
            line: "1",
            highlight: "1",
        });
    });

    it("adds data attributes for code nodes", () => {
        const tree = {
            type: "root",
            children: [
                {
                    type: "code",
                    lang: "ts",
                    meta: 'title="Hello.ts" highlight="1"',
                    value: "console.log('hi');",
                },
            ],
        };
        const plugin = remarkCodeBlockMeta();
        plugin(tree as any);

        const node = (tree as any).children[0];
        expect(node.data?.hProperties).toMatchObject({
            "data-language": "ts",
            "data-title": "Hello.ts",
            "data-highlight": "1",
            "data-meta": 'title="Hello.ts" highlight="1"',
        });
    });
});
