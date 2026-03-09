import React from "react";
import ReactDOM from "react-dom";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { appDataDir } from "@tauri-apps/api/path";
import { Alert, AlertDescription, AlertTitle } from "../components/ui/alert";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "../components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "../components/ui/dialog";
import { Input } from "../components/ui/input";
import {
    Select,
    SelectContent,
    SelectItem,
    SelectLabel,
    SelectSeparator,
    SelectTrigger,
    SelectValue,
} from "../components/ui/select";
import { Separator } from "../components/ui/separator";
import { Table, TableBody, TableCaption, TableCell, TableHead, TableHeader, TableRow } from "../components/ui/table";
import { Textarea } from "../components/ui/textarea";
import { markdownRegistry, type MarkdownTagRegistration } from "./markdownRegistry";

type PluginConstructor = new () => AippPlugin | AippAssistantTypePlugin;

interface BackendPluginItem {
    pluginId: number;
    name: string;
    version: string;
    code: string;
    pluginType: string[];
    permissions: string[];
    isActive: boolean;
}

interface PluginDataItem {
    dataId: number;
    pluginId: number;
    sessionId: string;
    dataKey: string;
    dataValue: string | null;
    createdAt: string;
    updatedAt: string;
}

interface AssistantSystemItem {
    id: number;
    name: string;
    assistant_type: number;
}

interface ModelSystemItem {
    id: number;
    name: string;
    code: string;
    llm_provider_id: number;
}

interface TextRunUsage {
    prompt_tokens?: number | null;
    completion_tokens?: number | null;
    total_tokens?: number | null;
}

interface TextRunResponse {
    content: string;
    model: string;
    usage?: TextRunUsage | null;
}

interface RunAssistantTextOptions {
    assistantId: number | string;
    prompt: string;
    systemPrompt?: string;
    context?: string;
}

interface RunModelTextOptions {
    modelId: string;
    prompt: string;
    systemPrompt?: string;
    context?: string;
}

type PluginThemeMode = "light" | "dark" | "both";

interface PluginThemeDefinition {
    id: string;
    label: string;
    mode?: PluginThemeMode;
    variables: Record<string, string>;
    description?: string;
    extraCss?: string;
    windowCss?: Record<string, string>;
}

interface RegisteredPluginTheme extends PluginThemeDefinition {
    ownerCode: string;
}

interface FeatureConfigListItem {
    id: number;
    feature_code: string;
    key: string;
    value: string;
}

interface DisplayConfigSnapshot {
    theme: string;
    color_mode: string;
    user_message_markdown_render: string;
    code_theme_light: string;
    code_theme_dark: string;
}

const BUILTIN_THEME_IDS = new Set<string>(["default", "newyear"]);
const PLUGIN_THEME_REGISTRY_STORAGE_KEY = "aipp-plugin-theme-registry";

interface StoredPluginThemeDefinition {
    label: string;
    mode: PluginThemeMode;
    variables: Record<string, string>;
    description?: string;
    extraCss?: string;
    windowCss?: Record<string, string>;
}

export interface LoadedPlugin {
    pluginId: number;
    name: string;
    version: string;
    code: string;
    pluginType: string[];
    instance: AippPlugin | AippAssistantTypePlugin | null;
}

class PluginRuntime {
    private plugins: LoadedPlugin[] = [];
    private loadPromise: Promise<LoadedPlugin[]> | null = null;
    private loadedScripts = new Set<string>();
    private pluginThemes = new Map<string, RegisteredPluginTheme>();

    async loadPlugins(forceReload = false): Promise<LoadedPlugin[]> {
        if (!forceReload && this.plugins.length > 0) {
            return this.plugins;
        }
        if (this.loadPromise) {
            return this.loadPromise;
        }

        this.loadPromise = this.loadPluginsInternal(forceReload).finally(() => {
            this.loadPromise = null;
        });
        return this.loadPromise;
    }

    async reloadPlugins(): Promise<LoadedPlugin[]> {
        return this.loadPlugins(true);
    }

    private async loadPluginsInternal(forceReload: boolean): Promise<LoadedPlugin[]> {
        this.exposeReactGlobals();
        const pluginItems = await invoke<BackendPluginItem[]>("get_enabled_plugins");
        const activeItems = pluginItems.filter((plugin) => plugin.isActive);
        const activeCodes = new Set(activeItems.map((plugin) => plugin.code));
        const baseDir = await appDataDir();
        const normalizedBaseDir = baseDir.endsWith("/") ? baseDir.slice(0, -1) : baseDir;
        const loaded: LoadedPlugin[] = [];

        if (forceReload) {
            this.plugins.forEach((loadedPlugin) => this.clearPluginThemesForPlugin(loadedPlugin.code));
            this.plugins.forEach((loadedPlugin) => this.clearMarkdownTagsForPlugin(loadedPlugin.code));
            this.plugins = [];
        }
        this.clearStalePluginThemes(activeCodes);
        this.clearStaleMarkdownTags(activeCodes);

        for (const plugin of activeItems) {
            try {
                const loadedPlugin = await this.loadSinglePlugin(plugin, normalizedBaseDir, forceReload);
                loaded.push(loadedPlugin);
            } catch (error) {
                console.error(`[PluginRuntime] Failed to load plugin '${plugin.code}':`, error);
                loaded.push({
                    pluginId: plugin.pluginId,
                    name: plugin.name,
                    version: plugin.version,
                    code: plugin.code,
                    pluginType: plugin.pluginType,
                    instance: null,
                });
            }
        }

        this.plugins = loaded;
        return loaded;
    }

    private exposeReactGlobals(): void {
        const globalWindow = window as Window & {
            React?: typeof React;
            ReactDOM?: typeof ReactDOM;
        };
        globalWindow.React = React;
        globalWindow.ReactDOM = ReactDOM;
    }

    private async loadSinglePlugin(
        plugin: BackendPluginItem,
        normalizedBaseDir: string,
        forceReload: boolean
    ): Promise<LoadedPlugin> {
        const pluginScriptPath = `${normalizedBaseDir}/plugin/${plugin.code}/dist/main.js`;
        if (forceReload) {
            this.loadedScripts.delete(pluginScriptPath);
            this.removeInjectedScripts(pluginScriptPath);
        }

        if (!this.loadedScripts.has(pluginScriptPath)) {
            this.clearPluginGlobals(plugin);
            const cacheBustKey = forceReload
                ? `${plugin.code}-${plugin.version}-${Date.now()}`
                : `${plugin.code}-${plugin.version}`;
            await this.injectScript(pluginScriptPath, cacheBustKey);
            this.loadedScripts.add(pluginScriptPath);
        }

        const PluginCtor = this.findPluginConstructor(plugin);
        if (!PluginCtor) {
            console.warn(`[PluginRuntime] No constructor found for plugin '${plugin.code}'`);
            return {
                pluginId: plugin.pluginId,
                name: plugin.name,
                version: plugin.version,
                code: plugin.code,
                pluginType: plugin.pluginType,
                instance: null,
            };
        }

        try {
            const instance = new PluginCtor();
            await this.callOnPluginLoad(instance, plugin);
            return {
                pluginId: plugin.pluginId,
                name: plugin.name,
                version: plugin.version,
                code: plugin.code,
                pluginType: plugin.pluginType,
                instance,
            };
        } catch (error) {
            console.error(`[PluginRuntime] Failed to instantiate plugin '${plugin.code}':`, error);
            return {
                pluginId: plugin.pluginId,
                name: plugin.name,
                version: plugin.version,
                code: plugin.code,
                pluginType: plugin.pluginType,
                instance: null,
            };
        }
    }

    private clearPluginGlobals(plugin: BackendPluginItem): void {
        const globalWindow = window as unknown as Record<string, unknown> & Window;
        const pascalCode = this.toPascalCase(plugin.code);
        const pascalName = this.toPascalCase(plugin.name || "");
        const keys = new Set<string>([
            plugin.code,
            pascalCode,
            `${pascalCode}Plugin`,
            pascalName,
            `${pascalName}Plugin`,
            "SamplePlugin",
        ]);

        keys.forEach((key) => {
            if (key) {
                globalWindow[key] = undefined;
            }
        });
    }

    private removeInjectedScripts(pluginScriptPath: string): void {
        const scripts = document.querySelectorAll<HTMLScriptElement>("script[data-plugin-script-path]");
        scripts.forEach((script) => {
            if (script.dataset.pluginScriptPath === pluginScriptPath) {
                script.remove();
            }
        });
    }

    private async callOnPluginLoad(
        instance: AippPlugin | AippAssistantTypePlugin,
        plugin: BackendPluginItem
    ): Promise<void> {
        const pluginInstance = instance as {
            onPluginLoad?: (systemApi: SystemApi) => void | Promise<void>;
        };
        if (typeof pluginInstance.onPluginLoad !== "function") {
            return;
        }
        const systemApi = this.createSystemApi(plugin);
        await Promise.resolve(pluginInstance.onPluginLoad(systemApi));
    }

    private createSystemApi(plugin: BackendPluginItem): SystemApi {
        const pluginId = plugin.pluginId;
        const pluginCode = plugin.code;

        return {
            pluginId,
            pluginCode,
            listAssistants: async () => invoke<AssistantSystemItem[]>("get_assistants"),
            listModels: async () => invoke<ModelSystemItem[]>("get_models_for_select"),
            getData: async (key: string, sessionId = "global") => {
                const dataMap = await this.getPluginDataMap(pluginId, sessionId);
                return dataMap.get(key) ?? null;
            },
            getAllData: async (sessionId = "global") => {
                const dataMap = await this.getPluginDataMap(pluginId, sessionId);
                const result: Record<string, string | null> = {};
                dataMap.forEach((value, key) => {
                    result[key] = value;
                });
                return result;
            },
            setData: async (key: string, value: string | null, sessionId = "global") => {
                await invoke("set_plugin_data", {
                    pluginId,
                    sessionId,
                    key,
                    value,
                });
            },
            runAssistantText: async (options: RunAssistantTextOptions) => {
                const assistantId = Number(options.assistantId);
                if (!Number.isFinite(assistantId) || assistantId <= 0) {
                    throw new Error("assistantId must be a positive number");
                }
                return invoke<TextRunResponse>("artifact_ai_ask", {
                    request: {
                        assistant_id: assistantId,
                        prompt: options.prompt,
                        system_prompt: options.systemPrompt,
                        context: options.context,
                    },
                });
            },
            runModelText: async (options: RunModelTextOptions) => {
                const modelId = String(options.modelId || "").trim();
                if (!modelId) {
                    throw new Error("modelId is required");
                }
                return invoke<TextRunResponse>("artifact_model_ask", {
                    request: {
                        model_id: modelId,
                        prompt: options.prompt,
                        system_prompt: options.systemPrompt,
                        context: options.context,
                    },
                });
            },
            registerTheme: (theme: PluginThemeDefinition) => {
                this.registerPluginTheme(plugin, theme);
            },
            unregisterTheme: (themeId: string) => {
                this.unregisterPluginThemeForOwner(plugin.code, themeId);
            },
            listThemes: async () => this.listDisplayThemes(),
            registerMarkdownTag: (registration: MarkdownTagRegistration) => {
                this.assertPluginPermission(plugin, "markdown.register");
                markdownRegistry.registerTag(plugin.code, registration);
            },
            unregisterMarkdownTag: (tagName: string) => {
                markdownRegistry.unregisterTag(plugin.code, tagName);
            },
            listMarkdownTags: async () =>
                markdownRegistry
                    .listTags()
                    .filter((tag) => tag.ownerCode === plugin.code)
                    .map((tag) => ({
                        tagName: tag.tagName,
                        attributes: [...tag.attributes],
                        render: tag.render,
                    })),
            getDisplayConfig: async () => this.getDisplayConfig(),
            applyTheme: async (themeId: string) => this.applyDisplayTheme(themeId),
            ui: {
                Alert,
                AlertDescription,
                AlertTitle,
                Badge,
                Button,
                Card,
                CardContent,
                CardDescription,
                CardFooter,
                CardHeader,
                CardTitle,
                Dialog,
                DialogContent,
                DialogDescription,
                DialogHeader,
                DialogTitle,
                Input,
                Select,
                SelectContent,
                SelectItem,
                SelectLabel,
                SelectSeparator,
                SelectTrigger,
                SelectValue,
                Separator,
                Table,
                TableBody,
                TableCaption,
                TableCell,
                TableHead,
                TableHeader,
                TableRow,
                Textarea,
            },
            invoke: async <T = unknown>(
                command: string,
                args?: Record<string, unknown>
            ): Promise<T> => invoke<T>(command, args ?? {}),
        };
    }

    async listDisplayThemes(): Promise<PluginThemeDefinition[]> {
        return [...this.pluginThemes.values()]
            .map((theme) => ({
                id: theme.id,
                label: theme.label,
                mode: theme.mode,
                variables: { ...theme.variables },
                description: theme.description,
                extraCss: theme.extraCss,
                windowCss: theme.windowCss ? { ...theme.windowCss } : undefined,
            }))
            .sort((a, b) => a.label.localeCompare(b.label));
    }

    private registerPluginTheme(plugin: BackendPluginItem, theme: PluginThemeDefinition): void {
        const themeId = this.normalizeThemeId(theme.id);
        if (!themeId) {
            throw new Error("theme.id is required");
        }
        const existing = this.pluginThemes.get(themeId);
        if (existing && existing.ownerCode !== plugin.code) {
            throw new Error(`theme.id '${themeId}' is already registered by plugin '${existing.ownerCode}'`);
        }
        const registeredTheme: RegisteredPluginTheme = {
            id: themeId,
            label: String(theme.label || "").trim() || themeId,
            mode: this.normalizeThemeMode(theme.mode),
            variables: this.normalizeThemeVariables(theme.variables),
            description: theme.description ? String(theme.description).trim() : undefined,
            extraCss: this.normalizeThemeExtraCss(theme.extraCss),
            windowCss: this.normalizeThemeWindowCss(theme.windowCss),
            ownerCode: plugin.code,
        };
        this.pluginThemes.set(themeId, registeredTheme);
        this.upsertThemeStyleElement(registeredTheme);
        this.persistPluginThemeRegistryToStorage();
    }

    private unregisterPluginThemeForOwner(pluginCode: string, themeId: string): void {
        const normalizedId = this.normalizeThemeId(themeId);
        const existing = this.pluginThemes.get(normalizedId);
        if (!existing || existing.ownerCode !== pluginCode) {
            return;
        }
        this.pluginThemes.delete(normalizedId);
        this.removeThemeStyleElement(normalizedId);
        this.persistPluginThemeRegistryToStorage();
    }

    private clearPluginThemesForPlugin(pluginCode: string): void {
        const themeIds = [...this.pluginThemes.entries()]
            .filter(([, theme]) => theme.ownerCode === pluginCode)
            .map(([themeId]) => themeId);
        themeIds.forEach((themeId) => {
            this.pluginThemes.delete(themeId);
            this.removeThemeStyleElement(themeId);
        });
        if (themeIds.length > 0) {
            this.persistPluginThemeRegistryToStorage();
        }
    }

    private clearMarkdownTagsForPlugin(pluginCode: string): void {
        markdownRegistry.clearTagsForPlugin(pluginCode);
    }

    private clearStalePluginThemes(activePluginCodes: Set<string>): void {
        const staleThemeIds = [...this.pluginThemes.entries()]
            .filter(([, theme]) => !activePluginCodes.has(theme.ownerCode))
            .map(([themeId]) => themeId);
        staleThemeIds.forEach((themeId) => {
            this.pluginThemes.delete(themeId);
            this.removeThemeStyleElement(themeId);
        });
        if (staleThemeIds.length > 0) {
            this.persistPluginThemeRegistryToStorage();
        }
    }

    private clearStaleMarkdownTags(activePluginCodes: Set<string>): void {
        markdownRegistry.clearStaleTags(activePluginCodes);
    }

    private assertPluginPermission(plugin: BackendPluginItem, permission: string): void {
        const permissions = Array.isArray(plugin.permissions) ? plugin.permissions : [];
        if (permissions.includes(permission)) {
            return;
        }
        throw new Error(`plugin '${plugin.code}' lacks required permission '${permission}'`);
    }

    private upsertThemeStyleElement(theme: RegisteredPluginTheme): void {
        const styleId = this.getThemeStyleElementId(theme.id);
        let styleElement = document.getElementById(styleId) as HTMLStyleElement | null;
        if (!styleElement) {
            styleElement = document.createElement("style");
            styleElement.id = styleId;
            styleElement.dataset.pluginTheme = theme.id;
            document.head.appendChild(styleElement);
        }
        styleElement.textContent = this.buildThemeCss(theme);
    }

    private removeThemeStyleElement(themeId: string): void {
        const styleElement = document.getElementById(this.getThemeStyleElementId(themeId));
        styleElement?.remove();
    }

    private getThemeStyleElementId(themeId: string): string {
        return `aipp-plugin-theme-${themeId}`;
    }

    private buildThemeCss(theme: RegisteredPluginTheme): string {
        const selector = this.getThemeSelector(theme);
        const declarations = Object.entries(theme.variables)
            .map(([name, value]) => `    ${name}: ${value};`)
            .join("\n");
        const baseRule = `${selector} {\n${declarations}\n}`;
        const extraCss = this.resolveThemeExtraCss(theme.extraCss, selector);
        const windowCss = this.resolveThemeWindowCss(theme.windowCss, selector);
        return [baseRule, extraCss, windowCss].filter(Boolean).join("\n");
    }

    private getThemeSelector(theme: RegisteredPluginTheme): string {
        const rootClass = `.theme-${theme.id}`;
        if (theme.mode === "dark") {
            return `${rootClass}.dark`;
        }
        if (theme.mode === "both") {
            return rootClass;
        }
        return `${rootClass}:not(.dark)`;
    }

    private normalizeThemeMode(mode?: string): PluginThemeMode {
        if (mode === "dark" || mode === "both") {
            return mode;
        }
        return "light";
    }

    private normalizeThemeExtraCss(extraCss?: string): string | undefined {
        if (typeof extraCss !== "string") {
            return undefined;
        }
        const normalized = extraCss.trim();
        return normalized || undefined;
    }

    private resolveThemeExtraCss(extraCss: string | undefined, selector: string): string {
        const normalized = this.normalizeThemeExtraCss(extraCss);
        if (!normalized) {
            return "";
        }
        if (normalized.includes(":scope")) {
            return normalized.replace(/:scope/g, selector);
        }
        return normalized;
    }

    private resolveThemeWindowCss(
        windowCss: Record<string, string> | undefined,
        selector: string
    ): string {
        if (!windowCss || typeof windowCss !== "object") {
            return "";
        }
        const scopedRules: string[] = [];
        Object.entries(windowCss).forEach(([rawWindowLabel, rawCss]) => {
            const windowLabel = this.normalizeWindowLabel(rawWindowLabel);
            const css = this.normalizeThemeExtraCss(rawCss);
            if (!windowLabel || !css) {
                return;
            }
            const windowScopeSelector = `${selector}.aipp-window-${windowLabel}`;
            if (css.includes(":scope")) {
                scopedRules.push(css.replace(/:scope/g, windowScopeSelector));
                return;
            }
            scopedRules.push(`${windowScopeSelector} ${css}`);
        });
        return scopedRules.join("\n");
    }

    private normalizeThemeVariables(variables: Record<string, string>): Record<string, string> {
        if (!variables || typeof variables !== "object") {
            throw new Error("theme.variables is required");
        }
        const normalized: Record<string, string> = {};
        Object.entries(variables).forEach(([rawName, rawValue]) => {
            const name = String(rawName || "").trim();
            const value = String(rawValue ?? "").trim();
            if (!name || !value) {
                return;
            }
            const cssVarName = name.startsWith("--") ? name : `--${name}`;
            normalized[cssVarName] = value;
        });
        if (Object.keys(normalized).length === 0) {
            throw new Error("theme.variables must contain at least one CSS variable");
        }
        return normalized;
    }

    private normalizeThemeWindowCss(windowCss?: Record<string, string>): Record<string, string> | undefined {
        if (!windowCss || typeof windowCss !== "object") {
            return undefined;
        }
        const normalized: Record<string, string> = {};
        Object.entries(windowCss).forEach(([rawWindowLabel, rawCss]) => {
            const windowLabel = this.normalizeWindowLabel(rawWindowLabel);
            const css = this.normalizeThemeExtraCss(rawCss);
            if (!windowLabel || !css) {
                return;
            }
            normalized[windowLabel] = css;
        });
        return Object.keys(normalized).length > 0 ? normalized : undefined;
    }

    private normalizeWindowLabel(rawWindowLabel: string): string {
        return String(rawWindowLabel || "")
            .trim()
            .toLowerCase()
            .replace(/[^a-z0-9_-]+/g, "-")
            .replace(/-{2,}/g, "-")
            .replace(/^[-_]+|[-_]+$/g, "");
    }

    private normalizeThemeId(rawThemeId: string): string {
        return String(rawThemeId || "")
            .trim()
            .toLowerCase()
            .replace(/[^a-z0-9_-]+/g, "-")
            .replace(/-{2,}/g, "-")
            .replace(/^[-_]+|[-_]+$/g, "");
    }

    private persistPluginThemeRegistryToStorage(): void {
        if (typeof window === "undefined") {
            return;
        }
        try {
            const storedRegistry: Record<string, StoredPluginThemeDefinition> = {};
            this.pluginThemes.forEach((theme) => {
                storedRegistry[theme.id] = {
                    label: theme.label,
                    mode: theme.mode || "light",
                    variables: { ...theme.variables },
                    description: theme.description,
                    extraCss: theme.extraCss,
                    windowCss: theme.windowCss ? { ...theme.windowCss } : undefined,
                };
            });
            window.localStorage.setItem(PLUGIN_THEME_REGISTRY_STORAGE_KEY, JSON.stringify(storedRegistry));
        } catch (error) {
            console.warn("[PluginRuntime] Failed to persist plugin theme registry:", error);
        }
    }

    private resolveIsDarkMode(colorMode: string): boolean {
        if (colorMode === "dark") {
            return true;
        }
        if (colorMode === "light") {
            return false;
        }
        if (typeof window !== "undefined" && typeof window.matchMedia === "function") {
            return window.matchMedia("(prefers-color-scheme: dark)").matches;
        }
        return false;
    }

    private applyThemeClassImmediate(themeName: string, colorMode: string): void {
        if (typeof document === "undefined") {
            return;
        }
        const root = document.documentElement;
        if (this.resolveIsDarkMode(colorMode)) {
            root.classList.add("dark");
        } else {
            root.classList.remove("dark");
        }

        [...root.classList].forEach((cls) => {
            if (cls.startsWith("theme-")) {
                root.classList.remove(cls);
            }
        });
        if (themeName && themeName !== "default") {
            root.classList.add(`theme-${themeName}`);
        }
    }

    private async getDisplayConfig(): Promise<DisplayConfigSnapshot> {
        const featureConfigList = await invoke<FeatureConfigListItem[]>("get_all_feature_config");
        const displayConfigMap = new Map<string, string>();
        featureConfigList
            .filter((item) => item.feature_code === "display")
            .forEach((item) => {
                displayConfigMap.set(item.key, item.value);
            });

        return {
            theme: displayConfigMap.get("theme") || "default",
            color_mode: displayConfigMap.get("color_mode") || "system",
            user_message_markdown_render: displayConfigMap.get("user_message_markdown_render") || "enabled",
            code_theme_light: displayConfigMap.get("code_theme_light") || "github",
            code_theme_dark: displayConfigMap.get("code_theme_dark") || "github-dark",
        };
    }

    private async applyDisplayTheme(themeId: string): Promise<void> {
        const normalizedThemeId = this.normalizeThemeId(themeId);
        const nextTheme = normalizedThemeId || "default";
        if (!BUILTIN_THEME_IDS.has(nextTheme) && !this.pluginThemes.has(nextTheme)) {
            throw new Error(`Theme '${nextTheme}' is not registered`);
        }
        const currentConfig = await this.getDisplayConfig();
        const nextConfig: DisplayConfigSnapshot = {
            ...currentConfig,
            theme: nextTheme,
        };
        await invoke("save_feature_config", {
            featureCode: "display",
            config: nextConfig,
        });
        if (typeof window !== "undefined") {
            window.localStorage.setItem("theme-mode", nextConfig.color_mode);
            window.localStorage.setItem("theme-name", nextTheme);
        }
        this.applyThemeClassImmediate(nextTheme, nextConfig.color_mode);
        await emit("theme-changed", {
            mode: nextConfig.color_mode,
            theme: nextTheme,
            code_theme_light: nextConfig.code_theme_light,
            code_theme_dark: nextConfig.code_theme_dark,
        });
    }

    private async getPluginDataMap(
        pluginId: number,
        sessionId: string
    ): Promise<Map<string, string | null>> {
        const rows = await invoke<PluginDataItem[]>("get_plugin_data", {
            pluginId,
            sessionId,
        });
        const dataMap = new Map<string, string | null>();
        for (const row of rows) {
            dataMap.set(row.dataKey, row.dataValue ?? null);
            }
        return dataMap;
    }

    private injectScript(pluginScriptPath: string, cacheBustKey?: string): Promise<void> {
        return new Promise((resolve, reject) => {
            const script = document.createElement("script");
            const src = convertFileSrc(pluginScriptPath);
            const withCacheBust = cacheBustKey
                ? `${src}${src.includes("?") ? "&" : "?"}v=${encodeURIComponent(cacheBustKey)}`
                : src;
            script.src = withCacheBust;
            script.dataset.pluginScriptPath = pluginScriptPath;
            script.onload = () => resolve();
            script.onerror = () =>
                reject(new Error(`[PluginRuntime] Failed to load plugin script: ${withCacheBust}`));
            document.body.appendChild(script);
        });
    }

    private findPluginConstructor(plugin: BackendPluginItem): PluginConstructor | null {
        const globalWindow = window as unknown as Record<string, unknown> & Window;
        const pascalCode = this.toPascalCase(plugin.code);
        const pascalName = this.toPascalCase(plugin.name || "");
        const candidates: unknown[] = [
            globalWindow[plugin.code],
            globalWindow[pascalCode],
            globalWindow[`${pascalCode}Plugin`],
            globalWindow[pascalName],
            globalWindow[`${pascalName}Plugin`],
            globalWindow.SamplePlugin,
        ];
        for (const candidate of candidates) {
            if (typeof candidate === "function") {
                return candidate as PluginConstructor;
            }
            if (candidate && typeof candidate === "object") {
                const defaultCtor = (candidate as Record<string, unknown>).default;
                if (typeof defaultCtor === "function") {
                    return defaultCtor as PluginConstructor;
                }
            }
        }
        return null;
    }

    private toPascalCase(value: string): string {
        return value
            .replace(/(^|[-_\s]+)([a-zA-Z0-9])/g, (_, __, c: string) => c.toUpperCase())
            .replace(/[^a-zA-Z0-9]/g, "");
    }
}

export const pluginRuntime = new PluginRuntime();
