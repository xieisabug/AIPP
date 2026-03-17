import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import RawTextRenderer from "./RawTextRenderer";

describe("RawTextRenderer skillattachment", () => {
    it("renders only user-facing skill metadata while hiding invocation and body", () => {
        render(
            <RawTextRenderer
                content={`前文\n<skillattachment skill_name="skill-creator" invocation="/skills(skill-creator)" identifier="agents:skill-creator"># Skill Creator\n请帮用户创建 Skill。</skillattachment>\n后文`}
            />,
        );

        expect(screen.getByText("skill-creator")).toBeInTheDocument();
        expect(screen.getByText("agents:skill-creator")).toBeInTheDocument();
        expect(screen.queryByText("/skills(skill-creator)")).not.toBeInTheDocument();
        expect(screen.queryByText(/请帮用户创建 Skill。/)).not.toBeInTheDocument();
        expect(screen.queryByText(/<skillattachment/i)).not.toBeInTheDocument();
    });

    it("does not leak multiline skillattachment body content into the message", () => {
        render(
            <RawTextRenderer
                content={`<skillattachment skill_name="AIPP Artifact" invocation="/skills(AIPP Artifact)" identifier="agents:aipp-artifact"># AIPP Artifact Skill Prompt（Workspace 版）\n\n你是 AIPP 的 Artifact 工作流助手。  \n\n</skillattachment>`}
            />,
        );

        expect(screen.getByText("AIPP Artifact")).toBeInTheDocument();
        expect(screen.getByText("agents:aipp-artifact")).toBeInTheDocument();
        expect(screen.queryByText("你是 AIPP 的 Artifact 工作流助手。")).not.toBeInTheDocument();
        expect(screen.queryByText("AIPP Artifact Skill Prompt（Workspace 版）")).not.toBeInTheDocument();
    });
});
