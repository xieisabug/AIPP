import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { avatarRegistry, type PluginAvatarAssetBundle } from "./avatarRegistry";
import AnimatedAvatar from "./AnimatedAvatar";
import AssistantProfileEditor from "@/components/butler/AssistantProfileEditor";
import { defaultAppearance, type ButlerAppearance } from "@/components/butler/butlerAppearance";

const sampleBundle: PluginAvatarAssetBundle = {
    avatars: [
        {
            id: "cute-pets:orange-cat",
            localId: "orange-cat",
            pluginCode: "cute-pets",
            pluginName: "萌宠包",
            label: "小猫",
            vectorParts: [
                { type: "path", d: "M100 100 H400 V400 H100 Z", fill: "secondary" },
                { type: "circle", cx: 255, cy: 255, r: 90, fill: "primary" },
                { type: "circle", cx: 255, cy: 300, r: 12, fill: "#FF8FA3" },
            ],
            defaultColors: { primary: "#FFF0B5", secondary: "#D87855" },
            faceColorSlot: "primary",
            face: { x: 255, y: 240, scale: 1.1 },
            crown: { x: 255, y: 80, scale: 0.95 },
            headphones: { x: 255, y: 70, scale: 0.95 },
            clip: { x: 140, y: 100, scale: 0.95, rotation: -20 },
            propDock: { x: 405, y: 350, scale: 0.8, rotation: -24 },
            colorLabels: { primary: "脸部", secondary: "毛色" },
            capabilities: ["eyes", "headwear"],
        },
    ],
    accessories: [
        {
            id: "cute-pets:star-glasses",
            localId: "star-glasses",
            pluginCode: "cute-pets",
            pluginName: "萌宠包",
            label: "星星眼镜",
            slot: "eyes",
            anchor: "face",
            followGaze: false,
            action: "glasses",
            previewViewBox: "-140 -65 280 130",
            layers: [
                { type: "rect", x: -80, y: -28, width: 60, height: 50, rx: 12, fill: "#FFE066", stroke: "#D9911A", strokeWidth: 6 },
            ],
        },
        {
            id: "cute-pets:wizard-hat",
            localId: "wizard-hat",
            pluginCode: "cute-pets",
            pluginName: "萌宠包",
            label: "魔法帽",
            slot: "head",
            anchor: "crown",
            followGaze: false,
            action: "hat",
            previewViewBox: "-140 -140 280 180",
            layers: [
                { type: "ellipse", cx: 0, cy: 10, rx: 120, ry: 20, fill: "#2C2E63" },
            ],
        },
    ],
};

describe("plugin avatar and accessory registry", () => {
    afterEach(() => {
        avatarRegistry.clearPluginAssets(false);
        cleanup();
    });

    it("should render plugin vector avatars and accessories with solid and slotted colors", () => {
        avatarRegistry.setPluginAssets(sampleBundle, false);
        const appearance: ButlerAppearance = {
            ...defaultAppearance,
            shape: "cute-pets:orange-cat",
            colors: { primary: "#FFF0B5", secondary: "#D87855" },
            accessories: { eyes: "cute-pets:star-glasses", head: "cute-pets:wizard-hat" },
        };
        const { container } = render(<AnimatedAvatar size={120} appearance={appearance} />);
        expect(container.firstChild).toHaveAttribute("data-avatar-shape", "cute-pets:orange-cat");
        expect(container.firstChild).not.toHaveAttribute("data-avatar-fallback");
        expect(container.querySelector('path[fill="#D87855"]')).toBeInTheDocument();
        expect(container.querySelector('circle[fill="#FFF0B5"]')).toBeInTheDocument();
        expect(container.querySelector('circle[fill="#FF8FA3"]')).toBeInTheDocument();
        expect(container.querySelector('[data-avatar-accessory="cute-pets:star-glasses"]')).toBeInTheDocument();
        expect(container.querySelector('[data-avatar-accessory="cute-pets:wizard-hat"]')).toBeInTheDocument();
    });

    it("should fall back gracefully to pebble when a plugin shape is uninstalled and restore when re-enabled", () => {
        const appearance: ButlerAppearance = {
            ...defaultAppearance,
            shape: "cute-pets:orange-cat",
            accessories: { eyes: "cute-pets:star-glasses", head: null },
        };
        const { container, rerender } = render(<AnimatedAvatar size={80} appearance={appearance} />);
        expect(container.firstChild).toHaveAttribute("data-avatar-shape", "cute-pets:orange-cat");
        expect(container.firstChild).toHaveAttribute("data-avatar-fallback", "true");
        expect(container.querySelector('[data-avatar-accessory="cute-pets:star-glasses"]')).toBeNull();

        avatarRegistry.setPluginAssets(sampleBundle, false);
        rerender(<AnimatedAvatar size={80} appearance={appearance} />);
        expect(container.firstChild).not.toHaveAttribute("data-avatar-fallback");
        expect(container.querySelector('[data-avatar-accessory="cute-pets:star-glasses"]')).toBeInTheDocument();
    });

    it("should list plugin avatars and accessories in the editor and apply recommended colors", () => {
        avatarRegistry.setPluginAssets(sampleBundle, false);
        const onChange = vi.fn();
        render(
            <AssistantProfileEditor
                name="测试助手"
                appearance={{ ...defaultAppearance, shape: "cute-pets:orange-cat" }}
                onAppearanceChange={onChange}
            />,
        );
        expect(screen.getAllByText("萌宠包").length).toBeGreaterThanOrEqual(1);
        expect(screen.getByRole("button", { name: "小猫" })).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "脸部：星星眼镜" })).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "头部：魔法帽" })).toBeInTheDocument();

        fireEvent.click(screen.getByRole("button", { name: "推荐配色" }));
        expect(onChange).toHaveBeenCalledWith(expect.objectContaining({
            colors: { primary: "#FFF0B5", secondary: "#D87855" },
        }));

        fireEvent.click(screen.getByRole("button", { name: "头部：魔法帽" }));
        expect(onChange).toHaveBeenCalledWith(expect.objectContaining({
            accessories: expect.objectContaining({ head: "cute-pets:wizard-hat" }),
        }));
    });

    it("should show a non-destructive notice when the selected plugin asset is missing", () => {
        render(
            <AssistantProfileEditor
                name="测试助手"
                appearance={{ ...defaultAppearance, shape: "missing-pack:fox" }}
                onAppearanceChange={vi.fn()}
            />,
        );
        expect(screen.getByRole("status")).toHaveTextContent("missing-pack");
    });

    it("should navigate to plugins-config when clicking more avatars", async () => {
        render(
            <AssistantProfileEditor
                name="测试助手"
                appearance={defaultAppearance}
                onAppearanceChange={vi.fn()}
            />,
        );
        fireEvent.click(screen.getByRole("button", { name: "更多形象" }));
        await waitFor(() => {
            expect(emit).toHaveBeenCalledWith("config-navigate-to", { menu: "plugins-config" });
            expect(invoke).toHaveBeenCalledWith("open_config_window");
        });
    });
});
