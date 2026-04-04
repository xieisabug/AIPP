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

    it("serializes display form switches back to persisted config values", () => {
        expect(
            serializeDisplayFormValues({
                theme: "default",
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
