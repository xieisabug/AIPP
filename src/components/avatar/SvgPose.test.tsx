import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { SvgPose } from "./SvgPose";

describe("SvgPose", () => {
    afterEach(cleanup);

    it("should start from the first keyframe as an SVG transform attribute around the local origin", () => {
        const { container } = render(<svg><SvgPose data-testid="pose" pose={{ x: [10, 50], rotate: [30, 0], scaleY: [0.5, 1] }} /></svg>);
        const group = container.querySelector("[data-testid=pose]")!;
        expect(group.getAttribute("transform")).toBe("translate(10 0) rotate(30) scale(1 0.5)");
        expect(group.getAttribute("style")).toBeNull();
    });
});
