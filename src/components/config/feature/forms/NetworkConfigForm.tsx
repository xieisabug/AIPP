import React, { useCallback, useEffect } from "react";
import { UseFormReturn } from "react-hook-form";
import ConfigForm from "@/components/ConfigForm";
import { toast } from "sonner";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";

interface CustomHeader {
    key: string;
    value: string;
}

interface NetworkConfigFormProps {
    form: UseFormReturn<any>;
    onSave: () => Promise<void>;
}

export const NetworkConfigForm: React.FC<NetworkConfigFormProps> = ({ form, onSave }) => {
    const { featureConfig, saveFeatureConfig } = useFeatureConfig();

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

            // 保存自定义 headers
            const customHeaders = form.getValues("custom_headers") || [];
            const headersMap: Record<string, string> = {};
            customHeaders.forEach(({ key, value }: CustomHeader) => {
                if (key.trim()) {
                    headersMap[key.trim()] = value;
                }
            });

            await saveFeatureConfig("network_config", { custom_headers: JSON.stringify(headersMap) });

            toast.success("网络配置保存成功");
        } catch (e) {
            toast.error("保存网络配置失败: " + e);
        }
    }, [onSave, form, saveFeatureConfig]);

    // 自定义 Headers 渲染函数
    const renderCustomHeaders = () => {
        const customHeaders = form.watch("custom_headers") || [];

        const addHeader = () => {
            const current = form.getValues("custom_headers") || [];
            form.setValue("custom_headers", [...current, { key: "", value: "" }]);
        };

        const removeHeader = (index: number) => {
            const current = form.getValues("custom_headers") || [];
            form.setValue("custom_headers", current.filter((_: any, i: number) => i !== index));
        };

        const updateHeader = (index: number, field: "key" | "value", value: string) => {
            const current = form.getValues("custom_headers") || [];
            const newHeaders = [...current];
            newHeaders[index] = { ...newHeaders[index], [field]: value };
            form.setValue("custom_headers", newHeaders);
        };

        return (
            <div className="space-y-3">
                {customHeaders.map((header: CustomHeader, index: number) => (
                    <div key={index} className="flex items-center gap-2">
                        <div className="flex-1">
                            <Input
                                placeholder="Header Key (如 User-Agent)"
                                value={header.key}
                                onChange={(e) => updateHeader(index, "key", e.target.value)}
                                className="bg-white dark:bg-gray-800"
                            />
                        </div>
                        <div className="flex-1">
                            <Input
                                placeholder="Header Value"
                                value={header.value}
                                onChange={(e) => updateHeader(index, "value", e.target.value)}
                                className="bg-white dark:bg-gray-800"
                            />
                        </div>
                        {customHeaders.length > 1 && (
                            <Button
                                variant="ghost"
                                size="icon"
                                onClick={() => removeHeader(index)}
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
    };

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
