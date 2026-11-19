import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { openUrl } from "@tauri-apps/plugin-opener";
import { MarkdownLink } from "./MarkdownLink";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn().mockResolvedValue(undefined) }));

it.each([
    ["https://gitlab.example/team/app/-/pipelines/32", "workflow"],
    ["./app.json?raw=true#L32", "braces"],
    ["./main.rs#L200", "code-xml"],
    ["https://example.org/", "globe"],
])("should show the target icon and open the original URL when clicked (%s)", (href, icon) => {
    render(<MarkdownLink href={href}>查看</MarkdownLink>);
    const link = screen.getByRole("link", { name: "查看" });
    expect(link.querySelector("svg")?.classList.contains(`lucide-${icon}`)).toBe(true);
    fireEvent.click(link);
    expect(openUrl).toHaveBeenCalledWith(href);
});
