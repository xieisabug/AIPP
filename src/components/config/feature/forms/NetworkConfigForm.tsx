import React, { useCallback, useEffect } from "react";
import { UseFormReturn, useFieldArray } from "react-hook-form";
import ConfigForm from "@/components/ConfigForm";
import { toast } from "sonner";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";

interface NetworkConfigFormProps {
    form: UseFormReturn<any>;
    onSave: () => Promise<void>;
}

export const NetworkConfigForm: React.FC<NetworkConfigFormProps> = ({ form, onSave }) => {
    const { featureConfig } = useFeatureConfig();

    // 使用 useFieldArray 管理 custom_headers 数组字段
    const { fields, append, remove } = useFieldArray({
        control: form.control,
        name: "custom_headers",
    });

    // 加载自定义 headers 配置到 form
    useEffect(() => {
        const headersConfig = featureConfig.get("network_config")?.get("custom_headers");
        if (headersConfig) {
            try {
                const parsed = JSON.parse(headersConfig) as Record<string, string>;
                const headers = Object.entries(parsed).map(([key, value]) => ({ key, value }));
                form.setValue("custom_headers", headers.length > 0 ? headers : [{ key: "", value: "" }]);
            } catch (e) {
                console.error("Failed to parse custom headers config", e);
            }
        } else {
            form.setValue("custom_headers", [{ key: "", value: "" }]);
        }
    }, [featureConfig, form]);

    const handleSaveNetwork = useCallback(async () => {
        try {
            await onSave();
            toast.success("网络配置保存成功");
        } catch (e) {
            toast.error("保存网络配置失败: " + e);
        }
    }, [onSave]);

    // 使用 useCallback 包装渲染函数，确保函数引用稳定
    const renderCustomHeaders = useCallback(() => {
        const addHeader = () => {
            append({ key: "", value: "" });
        };

        return (
            <div className="space-y-3">
                {fields.map((field, index) => (
                    <div key={field.id} className="flex items-center gap-2">
                        <div className="flex-1">
                            <Input
                                placeholder="Header Key (如 User-Agent)"
                                {...form.register(`custom_headers.${index}.key`)}
                                className="bg-white dark:bg-gray-800"
                            />
                        </div>
                        <div className="flex-1">
                            <Input
                                placeholder="Header Value"
                                {...form.register(`custom_headers.${index}.value`)}
                                className="bg-white dark:bg-gray-800"
                            />
                        </div>
                        {fields.length > 1 && (
                            <Button
                                variant="ghost"
                                size="icon"
                                onClick={() => remove(index)}
                                className="h-9 w-9 text-gray-500 hover:text-red-500 dark:text-gray-400"
                            >
                                <Trash2 className="h-4 w-4" />
                            </Button>
                        )}
                    </div>
                ))}
                {/* Add Button 放在 key-value input 下方 */}
                <div className="pt-2">
                    <Button
                        variant="outline"
                        size="sm"
                        onClick={addHeader}
                        className="h-8 px-3"
                    >
                        <Plus className="h-4 w-4 mr-1" />
                        添加 Header
                    </Button>
                </div>
            </div>
        );
    }, [fields, form, append, remove]);

    const NETWORK_FORM_CONFIG = [
        {
            key: "request_timeout",
            config: {
                type: "input" as const,
                label: "请求超时时间（秒）",
                placeholder: "180",
                description: "思考模型返回较慢，不建议设置过低",
            },
        },
        {
            key: "retry_attempts",
            config: {
                type: "input" as const,
                label: "失败重试次数",
                placeholder: "3",
                description: "请求失败时的重试次数",
            },
        },
        {
            key: "network_proxy",
            config: {
                type: "input" as const,
                label: "网络代理",
                placeholder: "http://127.0.0.1:7890",
                description: "支持 http、https 和 socks 协议，例如：http://127.0.0.1:7890",
            },
        },
        {
            key: "openai_prompt_cache_key_enabled",
            config: {
                type: "switch" as const,
                label: "OpenAI Responses 提示词缓存标识",
                tooltip: "为同一会话发送固定缓存标识。旧模型可用它优化缓存路由；GPT-5.6 及后续型号自动路由，标识可用于区分缓存统计。仅对 OpenAI / OpenAI API 类型且使用 Responses 模式的模型生效，默认开启。",
            },
        },
        {
            key: "openai_prompt_cache_ttl",
            config: {
                type: "select" as const,
                label: "OpenAI 提示词缓存保留时间",
                tooltip: "GPT-5.6 及后续型号的 Responses 模式使用新版最低缓存寿命参数，默认 30 分钟。30 分钟与 12 小时的缓存写入费分别为普通输入价格的 1.25 倍和 2.5 倍。关闭「OpenAI Responses 提示词缓存标识」后不发送缓存参数。",
                options: [
                    { value: "30m", label: "30 分钟（默认）" },
                    { value: "12h", label: "12 小时" },
                ],
            },
        },
        {
            key: "openai_prompt_cache_retention",
            config: {
                type: "select" as const,
                label: "OpenAI 旧版提示词缓存保留策略",
                tooltip: "旧模型或无法识别版本的自定义模型名称在 Responses 模式下沿用旧版参数，实际支持情况取决于模型和服务商。GPT-5.5 / GPT-5.5 Pro 仅支持 24 小时。内存缓存通常闲置 5～10 分钟后失效，最长约 1 小时；24 小时表示最长保留时间。关闭缓存标识后不发送此参数。",
                options: [
                    { value: "24h", label: "最长 24 小时" },
                    { value: "in_memory", label: "内存缓存" },
                ],
            },
        },
        {
            key: "openai_responses_stateful_enabled",
            config: {
                type: "switch" as const,
                label: "OpenAI Responses 服务端会话续接",
                tooltip:
                    "使用上一轮响应标识续接对话，由服务端保存并衔接历史，减少重复上传。仅对 OpenAI / OpenAI API 类型且使用 Responses 模式的模型生效，需先关闭「OpenAI Responses 提示词缓存标识」。历史输入仍参与计费，默认关闭。",
            },
        },
        {
            key: "custom_headers",
            config: {
                type: "custom" as const,
                label: "自定义 HTTP Headers",
                customRender: renderCustomHeaders,
            },
        },
    ];

    return (
        <>
            <ConfigForm
                title="网络配置"
                description="配置请求超时、重试次数和网络代理"
                config={NETWORK_FORM_CONFIG}
                layout="default"
                classNames="bottom-space"
                useFormReturn={form}
                onSave={handleSaveNetwork}
            />
        </>
    );
};

export default React.memo(NetworkConfigForm);
