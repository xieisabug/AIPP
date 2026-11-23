import { act, render, screen } from "@testing-library/react";
import { expect, it } from "vitest";
import { FitFrame, FRAME_HEIGHT_SCRIPT } from "./FitFrame";

function post(source: Window | null, data: unknown) {
    act(() => {
        window.dispatchEvent(new MessageEvent("message", { data, source }));
    });
}

it("should follow the reported content height when the message comes from its own frame", () => {
    render(<FitFrame title="preview" className="h-80" />);
    const frame = screen.getByTitle("preview") as HTMLIFrameElement;
    expect(frame.style.height).toBe("");
    post(frame.contentWindow, { type: "aipp-frame-height", height: 512.4 });
    expect(frame.style.height).toBe("513px");
    post(frame.contentWindow, { type: "aipp-frame-height", height: 120 });
    expect(frame.style.height).toBe("120px");
    expect(frame.className).toContain("max-h-[75vh]");
});

it("should ignore height messages when they come from another window or are malformed", () => {
    render(<FitFrame title="preview" />);
    const frame = screen.getByTitle("preview") as HTMLIFrameElement;
    post(window, { type: "aipp-frame-height", height: 900 });
    post(frame.contentWindow, { type: "aipp-frame-height", height: "tall" });
    post(frame.contentWindow, { type: "other", height: 900 });
    expect(frame.style.height).toBe("");
});

it("should ship the same height script that the backend injects", () => {
    expect(FRAME_HEIGHT_SCRIPT).toContain('type: "aipp-frame-height"');
    expect(FRAME_HEIGHT_SCRIPT).not.toContain("</script");
});
