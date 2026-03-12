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
});
