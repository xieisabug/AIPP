import React, { useCallback, useEffect, useRef, useState } from "react";
import { useFieldArray, useForm } from "react-hook-form";
import { Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ConfigPageLoading } from "@/components/common";
import { ConfigSaveFooter } from "@/components/common/ConfigSaveFooter";
import OptionalConfigSection from "@/components/config/OptionalConfigSection";
import { SettingsGroup, SettingRowText, settingRowClassName, stackedRowClassName } from "@/components/config/SettingsGroup";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import { useUnsavedLeaveGuard } from "@/hooks/useUnsavedLeaveGuard";
import { parseSubNav, useSettingsAnchorHighlight } from "@/lib/settingsNavigation";
import SettingsPageShell from "./SettingsPageShell";

interface NetworkFormValues {
    request_timeout: string;
    retry_attempts: string;
    network_proxy: string;
    openai_prompt_cache_key_enabled: boolean;
    openai_prompt_cache_ttl: string;
    openai_prompt_cache_retention: string;
    openai_responses_stateful_enabled: boolean;
    custom_headers: Array<{ key: string; value: string }>;
}

const DEFAULT_VALUES: NetworkFormValues = {
    request_timeout: "180",
    retry_attempts: "3",
    network_proxy: "",
    openai_prompt_cache_key_enabled: true,
    openai_prompt_cache_ttl: "30m",
    openai_prompt_cache_retention: "24h",
    openai_responses_stateful_enabled: false,
    custom_headers: [],
};

function parseNetworkConfig(config?: Map<string, string>): NetworkFormValues {
    const rawHeaders = config?.get("custom_headers");
    let custom_headers: Array<{ key: string; value: string }> = [];
    if (rawHeaders) {
        try {
            const parsed = JSON.parse(rawHeaders) as Record<string, string>;
            custom_headers = Object.entries(parsed).map(([key, value]) => ({ key, value }));
        } catch {
            custom_headers = [];
        }
    }
    return {
        request_timeout: config?.get("request_timeout") || "180",
        retry_attempts: config?.get("retry_attempts") || "3",
        network_proxy: config?.get("network_proxy") || "",
        openai_prompt_cache_key_enabled: (config?.get("openai_prompt_cache_key_enabled") ?? "true") !== "false",
        openai_prompt_cache_ttl: config?.get("openai_prompt_cache_ttl") || "30m",
        openai_prompt_cache_retention: config?.get("openai_prompt_cache_retention") || "24h",
        openai_responses_stateful_enabled: config?.get("openai_responses_stateful_enabled") === "true",
        custom_headers,
    };
}

interface NetworkProxyConfigProps {
    subNav?: string;
    onSubNavConsumed?: () => void;
}

export const NetworkProxyConfig: React.FC<NetworkProxyConfigProps> = ({
    subNav,
    onSubNavConsumed,
}) => {
    const { featureConfig, saveFeatureConfig, loading } = useFeatureConfig();
    const [initialLoaded, setInitialLoaded] = useState(false);
    const [saving, setSaving] = useState(false);
    const [anchor, setAnchor] = useState<string | null>(null);
    const baselineRef = useRef<NetworkFormValues>(DEFAULT_VALUES);

    const form = useForm<NetworkFormValues>({ defaultValues: DEFAULT_VALUES });
    const { fields, append, remove } = useFieldArray({ control: form.control, name: "custom_headers" });

    useEffect(() => {
        if (!subNav) return;
        const parsed = parseSubNav(subNav);
        if (parsed.anchor) setAnchor(parsed.anchor);
        onSubNavConsumed?.();
    }, [subNav, onSubNavConsumed]);

    useSettingsAnchorHighlight(anchor, () => setAnchor(null), String(initialLoaded));

    useEffect(() => {
        if (loading) return;
        const parsed = parseNetworkConfig(featureConfig.get("network_config"));
        baselineRef.current = structuredClone(parsed);
        form.reset(parsed);
        setInitialLoaded(true);
    }, [loading, featureConfig, form]);

    const watched = form.watch();
    const isDirty = initialLoaded && JSON.stringify(watched) !== JSON.stringify(baselineRef.current);

    const handleSave = useCallback(async () => {
        setSaving(true);
        try {
            const values = form.getValues();
            const headersMap: Record<string, string> = {};
            for (const item of values.custom_headers || []) {
                const k = item.key.trim();
                if (k) headersMap[k] = item.value ?? "";
            }
            await saveFeatureConfig("network_config", {
                request_timeout: String(values.request_timeout ?? "180"),
                retry_attempts: String(values.retry_attempts ?? "3"),
                network_proxy: String(values.network_proxy ?? ""),
                openai_prompt_cache_key_enabled: String(values.openai_prompt_cache_key_enabled),
                openai_prompt_cache_retention: String(values.openai_prompt_cache_retention || "24h"),
                openai_prompt_cache_ttl: String(values.openai_prompt_cache_ttl || "30m"),
                openai_responses_stateful_enabled: String(values.openai_responses_stateful_enabled),
                custom_headers: JSON.stringify(headersMap),
            });
            const normalized: NetworkFormValues = {
                ...values,
                custom_headers: Object.entries(headersMap).map(([key, value]) => ({ key, value })),
            };
            baselineRef.current = structuredClone(normalized);
            form.reset(normalized);
            toast.success("网络与代理配置已保存");
        } catch (e) {
            toast.error("保存网络配置失败: " + e);
            throw e;
        } finally {
            setSaving(false);
        }
    }, [form, saveFeatureConfig]);

    const handleDiscard = useCallback(() => {
        form.reset(structuredClone(baselineRef.current));
    }, [form]);

    const { dialog: leaveDialog } = useUnsavedLeaveGuard({
        isDirty,
        onSave: handleSave,
        onDiscard: handleDiscard,
    });

    if (!initialLoaded) {
        return <ConfigPageLoading />;
    }

    const cacheKeyEnabled = watched.openai_prompt_cache_key_enabled;
    const statefulEnabled = watched.openai_responses_stateful_enabled;

    return (
        <>
            <SettingsPageShell
                title="网络与代理"
                description="配置请求超时、重试策略、网络代理、自定义请求头与 OpenAI Responses 协议选项。"
                headerExtra={
                    isDirty ? (
                        <span className="rounded-full border border-foreground/30 bg-muted px-2.5 py-0.5 text-xs font-medium">
                            未保存
                        </span>
                    ) : null
                }
            >
                <SettingsGroup id="network-general" title="通用网络" description="应用于各模型请求及启用代理的提供商。">
                    <div className={settingRowClassName}>
                        <SettingRowText
                            label="请求超时时间（秒）"
                            description="思考模型返回较慢，不建议设置过低。"
                        />
                        <Input
                            type="number"
                            min={10}
                            className="w-36 shrink-0"
                            aria-label="请求超时时间（秒）"
                            {...form.register("request_timeout")}
                        />
                    </div>
                    <div className={settingRowClassName}>
                        <SettingRowText
                            label="失败重试次数"
                            description="请求遇到网络或临时错误时的自动重试次数。"
                        />
                        <Input
                            type="number"
                            min={0}
                            max={10}
                            className="w-36 shrink-0"
                            aria-label="失败重试次数"
                            {...form.register("retry_attempts")}
                        />
                    </div>
                    <div className={settingRowClassName}>
                        <SettingRowText
                            label="网络代理地址"
                            description="支持 http、https 和 socks5 协议，例如 http://127.0.0.1:7890。"
                        />
                        <Input
                            className="w-64 shrink-0"
                            placeholder="http://127.0.0.1:7890"
                            aria-label="网络代理地址"
                            {...form.register("network_proxy")}
                        />
                    </div>
                    <div className={stackedRowClassName}>
                        <div className="flex items-center justify-between gap-2">
                            <SettingRowText
                                label="自定义 HTTP Headers"
                                description="附加到模型 API 请求中的自定义请求头。"
                            />
                            <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                onClick={() => append({ key: "", value: "" })}
                            >
                                <Plus className="mr-1 h-3.5 w-3.5" />
                                添加 Header
                            </Button>
                        </div>
                        {fields.length === 0 ? (
                            <p className="text-xs text-muted-foreground">暂未配置自定义请求头</p>
                        ) : (
                            <div className="space-y-2 pt-1">
                                {fields.map((field, index) => (
                                    <div key={field.id} className="flex items-center gap-2">
                                        <Input
                                            placeholder="Header 名称（如 User-Agent）"
                                            {...form.register(`custom_headers.${index}.key`)}
                                        />
                                        <Input
                                            placeholder="Header 值"
                                            {...form.register(`custom_headers.${index}.value`)}
                                        />
                                        <Button
                                            type="button"
                                            variant="ghost"
                                            size="icon"
                                            aria-label="移除请求头"
                                            onClick={() => remove(index)}
                                            className="shrink-0 text-muted-foreground hover:text-destructive"
                                        >
                                            <Trash2 className="h-4 w-4" />
                                        </Button>
                                    </div>
                                ))}
                            </div>
                        )}
                    </div>
                </SettingsGroup>

                <div id="openai-responses">
                    <OptionalConfigSection
                        title="OpenAI Responses 协议高级选项"
                        description="仅对 OpenAI / OpenAI API 类型且使用 Responses 模式的模型生效。提示词缓存标识与服务端会话续接互斥。"
                    >
                        <SettingsGroup title="缓存与会话续接">
                            <div className={settingRowClassName}>
                                <SettingRowText
                                    label="OpenAI Responses 提示词缓存标识"
                                    description="为同一会话发送固定缓存标识以优化缓存路由与统计，默认开启。"
                                />
                                <Switch
                                    aria-label="OpenAI Responses 提示词缓存标识"
                                    checked={cacheKeyEnabled}
                                    onCheckedChange={(checked) => {
                                        form.setValue("openai_prompt_cache_key_enabled", checked);
                                        if (checked && statefulEnabled) {
                                            form.setValue("openai_responses_stateful_enabled", false);
                                        }
                                    }}
                                />
                            </div>
                            <div className={settingRowClassName}>
                                <SettingRowText
                                    label="提示词缓存保留时间（新版）"
                                    description="适用于 GPT-5.6 及后续型号（30m 写入 1.25x，12h 写入 2.5x）；关闭缓存标识时不发送。"
                                />
                                <div className="w-44 shrink-0">
                                    <Select
                                        disabled={!cacheKeyEnabled}
                                        value={watched.openai_prompt_cache_ttl}
                                        onValueChange={(v) => form.setValue("openai_prompt_cache_ttl", v)}
                                    >
                                        <SelectTrigger className="w-full" aria-label="提示词缓存保留时间">
                                            <SelectValue />
                                        </SelectTrigger>
                                        <SelectContent>
                                            <SelectItem value="30m">30 分钟（默认）</SelectItem>
                                            <SelectItem value="12h">12 小时</SelectItem>
                                        </SelectContent>
                                    </Select>
                                </div>
                            </div>
                            <div className={settingRowClassName}>
                                <SettingRowText
                                    label="旧版提示词缓存保留策略"
                                    description="适用于旧型号或自定义模型名（GPT-5.5 仅支持 24h）；关闭缓存标识时不发送。"
                                />
                                <div className="w-44 shrink-0">
                                    <Select
                                        disabled={!cacheKeyEnabled}
                                        value={watched.openai_prompt_cache_retention}
                                        onValueChange={(v) => form.setValue("openai_prompt_cache_retention", v)}
                                    >
                                        <SelectTrigger className="w-full" aria-label="旧版提示词缓存保留策略">
                                            <SelectValue />
                                        </SelectTrigger>
                                        <SelectContent>
                                            <SelectItem value="24h">最长 24 小时</SelectItem>
                                            <SelectItem value="in_memory">内存缓存</SelectItem>
                                        </SelectContent>
                                    </Select>
                                </div>
                            </div>
                            <div className={settingRowClassName}>
                                <SettingRowText
                                    label="服务端会话续接（Stateful）"
                                    description="由服务端衔接历史减少重复上传。开启时会自动关闭上方的「提示词缓存标识」。"
                                />
                                <Switch
                                    aria-label="服务端会话续接"
                                    checked={statefulEnabled}
                                    onCheckedChange={(checked) => {
                                        form.setValue("openai_responses_stateful_enabled", checked);
                                        if (checked && cacheKeyEnabled) {
                                            form.setValue("openai_prompt_cache_key_enabled", false);
                                        }
                                    }}
                                />
                            </div>
                        </SettingsGroup>
                    </OptionalConfigSection>
                </div>

                <ConfigSaveFooter>
                    <div className="flex items-center justify-between gap-3">
                        <div className="flex items-center gap-2">
                            <Button type="button" disabled={saving || !isDirty} onClick={() => void handleSave()}>
                                {saving ? "保存中…" : "保存配置"}
                            </Button>
                            {isDirty ? (
                                <Button type="button" variant="outline" disabled={saving} onClick={handleDiscard}>
                                    放弃更改
                                </Button>
                            ) : null}
                        </div>
                        {isDirty ? (
                            <span className="text-xs text-muted-foreground">有未保存的更改</span>
                        ) : null}
                    </div>
                </ConfigSaveFooter>
            </SettingsPageShell>
            {leaveDialog}
        </>
    );
};

export default NetworkProxyConfig;
