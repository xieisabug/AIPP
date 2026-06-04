import { describe, expect, it } from "vitest";
import {
    buildDisplayFormValues,
    serializeDisplayFormValues,
} from "./displayConfigFormValues";

describe("displayConfigFormValues", () => {
    it("builds display form values with the new switches reflected correctly", () => {
        const config = new Map<string, string>([
            ["theme", "default"],
            ["color_mode", "dark"],
            ["user_message_markdown_render", "enabled"],
            ["notification_on_completion", "true"],
            ["code_theme_light", "github"],
            ["code_theme_dark", "github-dark"],
            ["merge_assistant_messages", "disabled"],
            ["show_thinking", "disabled"],
            ["preview_code_show_toolbar", "enabled"],
        ]);

        expect(buildDisplayFormValues(config)).toEqual({
            default_home_window: "ask",
            theme: "default",
            color_mode: "dark",
            user_message_markdown_render: "enabled",
            notification_on_completion: true,
            code_theme_light: "github",
            code_theme_dark: "github-dark",
            merge_assistant_messages: false,
            show_thinking: false,
            preview_code_show_toolbar: true,
        });
    });

    it("reads the default home window from experimental config", () => {
        const displayConfig = new Map<string, string>();
        const experimentalConfig = new Map<string, string>([
            ["butler_experiment_enabled", "true"],
            ["default_home_window", "butler_experiment"],
        ]);

        expect(buildDisplayFormValues(displayConfig, experimentalConfig).default_home_window)
            .toBe("butler_experiment");
    });

    it("hides the butler home window value when butler mode is disabled", () => {
        const displayConfig = new Map<string, string>();
        const experimentalConfig = new Map<string, string>([
            ["butler_experiment_enabled", "false"],
            ["default_home_window", "butler_experiment"],
        ]);

        expect(buildDisplayFormValues(displayConfig, experimentalConfig).default_home_window)
            .toBe("ask");
    });

    it("serializes display form switches back to persisted config values", () => {
        expect(
            serializeDisplayFormValues({
                theme: "default",
                default_home_window: "chat_ui",
                color_mode: "system",
                user_message_markdown_render: "enabled",
                notification_on_completion: false,
                code_theme_light: "github",
                code_theme_dark: "github-dark",
                merge_assistant_messages: true,
                show_thinking: false,
                preview_code_show_toolbar: true,
            }),
        ).toEqual({
            theme: "default",
            color_mode: "system",
            user_message_markdown_render: "enabled",
            notification_on_completion: "false",
            code_theme_light: "github",
            code_theme_dark: "github-dark",
            merge_assistant_messages: "enabled",
            show_thinking: "disabled",
            preview_code_show_toolbar: "enabled",
        });
    });
});
