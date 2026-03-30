import React, { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import {
    CheckCircle2,
    ExternalLink,
    Loader2,
    MessageSquare,
    RefreshCw,
    XCircle,
} from "lucide-react";

interface FeishuRuntimeStatus {
    butler_enabled: boolean;
    enabled: boolean;
    configured: boolean;
    secret_configured: boolean;
    running: boolean;
    connected: boolean;
    app_id?: string | null;
    base_url?: string | null;
    last_error?: string | null;
    status_text: string;
}

interface StepFeishuGuideProps {
    feishuEnabled: boolean;
    feishuAppId: string;
    feishuAppSecret: string;
    feishuBaseUrl: string;
    onEnabledChange: (enabled: boolean) => void;
    onAppIdChange: (appId: string) => void;
    onAppSecretChange: (secret: string) => void;
    onBaseUrlChange: (url: string) => void;
}

const FEISHU_GUIDE_STEPS = [
    {
        title: "登录飞书开放平台",
        content: "访问 open.feishu.cn，使用管理员账号登录飞书开放平台。",
        link: "https://open.feishu.cn",
    },
    {
        title: "创建企业自建应用",
        content: '点击"创建应用" → 选择"企业自建应用" → 填写应用名称和描述。',
    },
    {
        title: "添加机器人能力",
        content: '进入应用详情 → 左侧菜单"添加应用能力" → 选择"机器人"。',
    },
    {
        title: "开启长连接模式",
        content: '左侧菜单"事件与回调" → "事件配置方式"选择"使用长连接接收事件"。',
    },
    {
        title: "获取凭证",
        content: '在"凭证与基础信息"页获取 App ID 和 App Secret，填入下方配置。',
    },
    {
        title: "发布应用",
        content: '完成配置后点击"版本管理与发布" → "创建版本" → 提交审核发布。',
    },
];

const StepFeishuGuide: React.FC<StepFeishuGuideProps> = ({
    feishuEnabled,
    feishuAppId,
    feishuAppSecret,
    feishuBaseUrl,
    onEnabledChange,
    onAppIdChange,
    onAppSecretChange,
    onBaseUrlChange,
}) => {
    const [feishuStatus, setFeishuStatus] = useState<FeishuRuntimeStatus | null>(null);
    const [loadingStatus, setLoadingStatus] = useState(false);

    const fetchFeishuStatus = useCallback(async () => {
        setLoadingStatus(true);
        try {
            const status = await invoke<FeishuRuntimeStatus>("get_butler_feishu_runtime_status");
            setFeishuStatus(status);
        } catch {
            // Feishu not configured yet
        } finally {
            setLoadingStatus(false);
        }
    }, []);

    useEffect(() => {
        void fetchFeishuStatus();
    }, [fetchFeishuStatus]);

    useEffect(() => {
        const unlisten = listen<FeishuRuntimeStatus>("butler_feishu_status_changed", (event) => {
            setFeishuStatus(event.payload);
        });
        return () => {
            unlisten.then((f) => f());
        };
    }, []);

    return (
        <div className="space-y-6">
            <div className="space-y-2">
                <div className="flex items-center gap-2 text-primary">
                    <MessageSquare className="h-5 w-5" />
                    <h3 className="text-lg font-semibold">飞书机器人接入</h3>
                </div>
                <p className="text-sm text-muted-foreground leading-relaxed">
                    连接飞书后，你可以通过飞书消息直接与总管家对话。这是一个可选步骤，
                    你可以稍后在设置中配置。
                </p>
            </div>

            {/* Enable toggle */}
            <div className="flex items-center justify-between rounded-lg border border-border/60 bg-muted/30 p-4">
                <div className="space-y-1">
                    <span className="text-sm font-medium">启用飞书机器人</span>
                    <p className="text-xs text-muted-foreground">
                        开启后总管家会通过飞书长连接接收文本消息。
                    </p>
                </div>
                <Switch checked={feishuEnabled} onCheckedChange={onEnabledChange} />
            </div>

            {feishuEnabled && (
                <>
                    {/* Guide steps */}
                    <div className="rounded-lg border border-border/60 bg-muted/30 p-4 space-y-3">
                        <p className="text-sm font-medium">创建飞书应用指引</p>
                        <ol className="space-y-2.5">
                            {FEISHU_GUIDE_STEPS.map((step, index) => (
                                <li key={index} className="flex gap-3">
                                    <span className="shrink-0 w-6 h-6 rounded-full bg-primary/10 text-primary text-xs font-medium flex items-center justify-center mt-0.5">
                                        {index + 1}
                                    </span>
                                    <div className="space-y-0.5">
                                        <p className="text-sm font-medium">{step.title}</p>
                                        <p className="text-xs text-muted-foreground">{step.content}</p>
                                        {step.link && (
                                            <a
                                                href={step.link}
                                                target="_blank"
                                                rel="noopener noreferrer"
                                                className="inline-flex items-center gap-1 text-xs text-primary hover:underline"
                                                onClick={(e) => {
                                                    e.preventDefault();
                                                    void openUrl(step.link!);
                                                }}
                                            >
                                                打开飞书开放平台
                                                <ExternalLink className="h-3 w-3" />
                                            </a>
                                        )}
                                    </div>
                                </li>
                            ))}
                        </ol>
                    </div>

                    {/* Config form */}
                    <div className="rounded-lg border border-border/60 bg-muted/30 p-4 space-y-4">
                        <p className="text-sm font-medium">飞书应用配置</p>

                        <div className="space-y-2">
                            <label className="text-sm font-medium">App ID</label>
                            <Input
                                value={feishuAppId}
                                onChange={(e) => onAppIdChange(e.target.value)}
                                placeholder="cli_xxx"
                            />
                        </div>

                        <div className="space-y-2">
                            <label className="text-sm font-medium">App Secret</label>
                            <Input
                                type="password"
                                value={feishuAppSecret}
                                onChange={(e) => onAppSecretChange(e.target.value)}
                                placeholder={
                                    feishuStatus?.secret_configured
                                        ? "已保存，留空则保持不变"
                                        : "输入 App Secret"
                                }
                            />
                            <p className="text-xs text-muted-foreground">
                                Secret 会单独加密保存，不会明文存储。
                            </p>
                        </div>

                        <div className="space-y-2">
                            <label className="text-sm font-medium">飞书开放平台域名</label>
                            <Select value={feishuBaseUrl} onValueChange={onBaseUrlChange}>
                                <SelectTrigger>
                                    <SelectValue placeholder="选择域名" />
                                </SelectTrigger>
                                <SelectContent>
                                    <SelectItem value="https://open.feishu.cn">
                                        飞书（中国大陆）
                                    </SelectItem>
                                    <SelectItem value="https://open.larksuite.com">
                                        Lark（国际版）
                                    </SelectItem>
                                </SelectContent>
                            </Select>
                        </div>

                        {/* Connection status */}
                        {feishuStatus && (
                            <div className="flex items-center justify-between pt-2 border-t border-border/40">
                                <div className="flex items-center gap-2 text-sm">
                                    {loadingStatus ? (
                                        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                                    ) : feishuStatus.connected ? (
                                        <CheckCircle2 className="h-4 w-4 text-green-600 dark:text-green-400" />
                                    ) : (
                                        <XCircle className="h-4 w-4 text-muted-foreground" />
                                    )}
                                    <span className="text-muted-foreground">
                                        {feishuStatus.status_text}
                                    </span>
                                </div>
                                <Button
                                    type="button"
                                    variant="ghost"
                                    size="sm"
                                    onClick={() => void fetchFeishuStatus()}
                                >
                                    <RefreshCw className="h-3.5 w-3.5" />
                                </Button>
                            </div>
                        )}
                    </div>
                </>
            )}

            {!feishuEnabled && (
                <div className="text-center py-4 text-sm text-muted-foreground">
                    如需稍后接入飞书，可在总管家设置中随时开启。
                </div>
            )}
        </div>
    );
};

export default StepFeishuGuide;
