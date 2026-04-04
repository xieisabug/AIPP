export interface DisplayFormValues {
    theme: string;
    color_mode: string;
    user_message_markdown_render: string;
    notification_on_completion: boolean;
    code_theme_light: string;
    code_theme_dark: string;
    merge_assistant_messages: boolean;
    show_thinking: boolean;
    preview_code_show_toolbar: boolean;
}

export function buildDisplayFormValues(
    displayConfig?: Map<string, string>,
): DisplayFormValues {
    return {
        theme: displayConfig?.get("theme") || "default",
        color_mode: displayConfig?.get("color_mode") || "system",
        user_message_markdown_render:
            displayConfig?.get("user_message_markdown_render") || "disabled",
        notification_on_completion:
            displayConfig?.get("notification_on_completion") === "true",
        code_theme_light: displayConfig?.get("code_theme_light") || "github",
        code_theme_dark: displayConfig?.get("code_theme_dark") || "github-dark",
        merge_assistant_messages:
            displayConfig?.get("merge_assistant_messages") !== "disabled",
        show_thinking: displayConfig?.get("show_thinking") !== "disabled",
        preview_code_show_toolbar:
            displayConfig?.get("preview_code_show_toolbar") === "enabled",
    };
}

export function serializeDisplayFormValues(values: DisplayFormValues) {
    return {
        theme: values.theme,
        color_mode: values.color_mode,
        user_message_markdown_render: values.user_message_markdown_render,
        notification_on_completion: values.notification_on_completion.toString(),
        code_theme_light: values.code_theme_light,
        code_theme_dark: values.code_theme_dark,
        merge_assistant_messages: values.merge_assistant_messages
            ? "enabled"
            : "disabled",
        show_thinking: values.show_thinking ? "enabled" : "disabled",
        preview_code_show_toolbar: values.preview_code_show_toolbar
            ? "enabled"
            : "disabled",
    };
}
