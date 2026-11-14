import { render, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { accessoryDefinitions } from "./avatarDefinition";
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

    it("should tilt the equipped crown in place and settle it upright", async () => {
        const { container } = render(<AnimatedAvatar animated size={160}
            appearance={{ ...defaultAppearance, accessories: { eyes: null, head: "crown" } }}
            actionRequest={{ action: accessoryDefinitions.crown.action, sequence: 1 }} />);
        const crown = container.querySelector('[data-avatar-accessory="crown"]')!.firstElementChild;
        await waitFor(() => expect(crown).toHaveAttribute("transform", "translate(0 0) rotate(30) scale(1 1)"));
        const gaze = container.querySelector('[data-avatar-gaze="crown"]');
        await waitFor(() => expect(gaze).toHaveAttribute("transform", "translate(13 -18) rotate(0) scale(1 1)"));
        expect(crown).toHaveAttribute("transform", "translate(0 0) rotate(30) scale(1 1)");
        await waitFor(() => expect(container.firstChild).toHaveAttribute("data-avatar-action", "none"), { timeout: 2200 });
        expect(crown).toHaveAttribute("transform", "translate(0 0) rotate(0) scale(1 1)");
    });

    it("should serve rice for a whale smile and put the bowl away when the meal finishes", async () => {
        const { container } = render(<AnimatedAvatar animated size={200}
            appearance={{ ...defaultAppearance, shape: "whale" }} actionRequest={{ action: "smile", sequence: 1 }} />);
        const bowl = () => container.querySelector('[data-avatar-prop="rice-meal"]');
        expect(bowl()).toBeInTheDocument();
        await waitFor(() => expect(bowl()).toBeNull(), { timeout: 3300 });
        expect(container.firstChild).toHaveAttribute("data-avatar-action", "none");
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
