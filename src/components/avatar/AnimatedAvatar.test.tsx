import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import AnimatedAvatar from "./AnimatedAvatar";
import { defaultAppearance } from "@/components/butler/butlerAppearance";

describe("avatar rendering", () => {
    it("should show the terminal expression only while the blossom is coding", () => {
        const appearance = { ...defaultAppearance, shape: "blossom" as const };
        const { container, rerender } = render(<AnimatedAvatar animated appearance={appearance} state="coding" />);
        const expression = container.querySelector('[data-avatar-expression="coding"]');
        expect(expression).toHaveAttribute("opacity", "1");
        rerender(<AnimatedAvatar animated appearance={appearance} state="idle" />);
        expect(expression).toHaveAttribute("opacity", "0");
        rerender(<AnimatedAvatar animated appearance={appearance} actionRequest={{ action: "smile", sequence: 1 }} />);
        expect(container.firstChild).toHaveAttribute("data-avatar-action", "smile");
        expect(expression).toHaveAttribute("opacity", "0");
    });

    it.each(["#4D6BFE", "#000000", "#D87855"])("should keep both eyes white on the whale when its body is %s", color => {
        const { container } = render(<AnimatedAvatar appearance={{ ...defaultAppearance, shape: "whale",
            colors: { ...defaultAppearance.colors, secondary: color }, accessories: { eyes: "blush", head: "flower" } }} />);
        expect(container.querySelectorAll(`path[fill="#FFFFFF"][stroke="${color}"]`)).toHaveLength(2);
        expect(container.querySelectorAll("[data-avatar-accessory]")).toHaveLength(2);
    });

    it("should render both original avatars in their selected solid colors", () => {
        const colors = { primary: "#CEF3DF", secondary: "#193B68" };
        const { container } = render(<><AnimatedAvatar appearance={{ ...defaultAppearance, colors }} />
            <AnimatedAvatar appearance={{ ...defaultAppearance, shape: "pebble", colors, accessories: { eyes: "round-glasses", head: "beret" } }} /></>);
        for (const avatar of container.querySelectorAll("svg")) {
            expect(avatar.querySelector(`path[fill="${colors.primary}"]`)).toBeInTheDocument();
            expect(avatar.querySelector(`path[fill="${colors.secondary}"]`)).toBeInTheDocument();
            expect(avatar.querySelector('[fill^="url("]')).toBeNull();
        }
        expect(container.querySelectorAll('[data-avatar-accessory="round-glasses"]')).toHaveLength(1);
        expect(container.querySelectorAll('[data-avatar-accessory="beret"]')).toHaveLength(1);
        expect(container.querySelectorAll('[data-avatar-active="false"]')).toHaveLength(2);
    });
});
