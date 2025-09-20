import { visit } from "unist-util-visit";
import { Node } from "unist";

interface TextNode extends Node {
    type: "text";
    value: string;
}

interface Parent extends Node {
    children: Node[];
}

export default function remarkCustomCompenent() {
    return (tree: Node) => {
        visit(
            tree,
            "text",
            (node: TextNode, index: number | null, parent: Parent | null) => {
                const { value } = node;
                const match = value.match(/^@tips:(.*)/);

                if (match) {
                    const text = match[1].trim();

                    parent?.children.splice(index as number, 1, {
                        type: "tips",
                        data: {
                            // 使用与 ReactMarkdown 组件映射一致的小写自定义标签名
                            // 对应 constants/markdown.ts 中的 MARKDOWN_COMPONENTS_BASE.tipscomponent
                            hName: "tipscomponent",
                            hProperties: { text },
                        },
                    });
                }
            },
        );
    };
}
