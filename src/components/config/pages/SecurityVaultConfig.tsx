import React, { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, Check, KeyRound, Shield } from "lucide-react";
import { toast } from "sonner";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Switch } from "@/components/ui/switch";
import VaultConfig from "@/components/config/VaultConfig";
import { SettingsGroup, SettingRowText, settingRowClassName } from "@/components/config/SettingsGroup";
import SettingsLink from "@/components/config/SettingsLink";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import { parseSubNav, useSettingsAnchorHighlight } from "@/lib/settingsNavigation";

interface SecurityVaultConfigProps {
    navigateTo?: (menuKey: string, subNav?: string) => void;
    subNav?: string;
    onSubNavConsumed?: () => void;
}

interface SyncSecuritySnapshot {
    mode?: string;
    secret_sync_state?: string;
    secret_sync_message?: string;
}

export const SecurityVaultConfig: React.FC<SecurityVaultConfigProps> = ({
    navigateTo,
    subNav,
    onSubNavConsumed,
}) => {
    const [tab, setTab] = useState<"vault" | "policies">("vault");
    const [anchor, setAnchor] = useState<string | null>(null);
    const { featureConfig, saveFeatureConfig, loading } = useFeatureConfig();
    const [antiLeakageBusy, setAntiLeakageBusy] = useState(false);
    const [savedPulse, setSavedPulse] = useState(false);
    const [syncSnapshot, setSyncSnapshot] = useState<SyncSecuritySnapshot | null>(null);

    useEffect(() => {
        if (!subNav) return;
        const parsed = parseSubNav(subNav);
        if (parsed.tab === "policies" || parsed.anchor === "anti-leakage" || parsed.anchor === "security-overview") {
            setTab("policies");
        } else if (parsed.tab === "vault") {
            setTab("vault");
        }
        if (parsed.anchor) setAnchor(parsed.anchor);
        onSubNavConsumed?.();
    }, [subNav, onSubNavConsumed]);

    useSettingsAnchorHighlight(anchor, () => setAnchor(null), tab);

    useEffect(() => {
        if (tab !== "policies") return;
        let active = true;
        invoke<SyncSecuritySnapshot>("get_sync_status")
            .then((status) => {
                if (active) setSyncSnapshot(status);
            })
            .catch(() => {
                if (active) setSyncSnapshot(null);
            });
        return () => { active = false; };
    }, [tab]);

    const antiLeakageEnabled = featureConfig.get("anti_leakage")?.get("enabled") === "true";
    const trustAllWorkspaces = featureConfig.get("experimental")?.get("butler_trust_all_workspaces") === "true";
    const butlerEnabled = featureConfig.get("experimental")?.get("butler_experiment_enabled") === "true";
    const autoReviewModel = featureConfig.get("conversation_summary")?.get("auto_review_model") || "";
    const defaultDecisionModel = featureConfig.get("conversation_summary")?.get("default_decision_model") || "";
    const effectiveReviewModel = autoReviewModel || defaultDecisionModel;

    const handleAntiLeakageChange = useCallback(
        async (checked: boolean) => {
            setAntiLeakageBusy(true);
            try {
                await saveFeatureConfig("anti_leakage", { enabled: checked ? "true" : "false" });
                setSavedPulse(true);
                window.setTimeout(() => setSavedPulse(false), 1500);
            } catch (e) {
                toast.error("设置防泄露模式失败: " + e);
            } finally {
                setAntiLeakageBusy(false);
            }
        },
        [saveFeatureConfig]
    );

    return (
        <div className="flex h-full min-h-0 flex-col bg-background">
            <Tabs
                value={tab}
                onValueChange={(next) => setTab(next as "vault" | "policies")}
                className="flex min-h-0 flex-1 flex-col gap-0"
            >
                <div className="shrink-0 border-b border-border px-4 pt-5 pb-3 sm:px-6">
                    <div className="flex flex-wrap items-center justify-between gap-3">
                        <div>
                            <h2 className="text-xl font-semibold tracking-tight">安全与凭据</h2>
                            <p className="mt-1 text-sm text-muted-foreground">
                                集中管理加密凭据保险箱、界面防泄露保护与全局安全策略状态。
                            </p>
                        </div>
                        <TabsList>
                            <TabsTrigger value="vault" className="gap-1.5">
                                <KeyRound className="h-3.5 w-3.5" />
                                凭据保险箱
                            </TabsTrigger>
                            <TabsTrigger value="policies" className="gap-1.5">
                                <Shield className="h-3.5 w-3.5" />
                                隐私与安全策略
                            </TabsTrigger>
                        </TabsList>
                    </div>
                </div>

                <TabsContent value="vault" className="mt-0 min-h-0 flex-1 overflow-hidden">
                    <VaultConfig navigateTo={navigateTo} />
                </TabsContent>

                <TabsContent value="policies" className="mt-0 min-h-0 flex-1 overflow-y-auto">
                    <div className="w-full space-y-6 px-4 py-6 sm:px-6">
                        {butlerEnabled && trustAllWorkspaces ? (
                            <div
                                role="alert"
                                className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-destructive/40 bg-destructive/5 px-4 py-3 text-sm"
                            >
                                <div className="flex items-start gap-2.5">
                                    <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-destructive" />
                                    <div>
                                        <div className="font-medium text-destructive">总管家当前已开启「信任任何工作区」</div>
                                        <p className="mt-0.5 text-xs text-muted-foreground">
                                            所有路径的文件操作均会自动放行且不弹出确认，请仅在完全受信环境使用。
                                        </p>
                                    </div>
                                </div>
                                <SettingsLink
                                    menu="butler-config"
                                    subNav="workspaces#butler-workspaces"
                                    navigateTo={navigateTo}
                                >
                                    前往调整工作区
                                </SettingsLink>
                            </div>
                        ) : null}

                        <SettingsGroup
                            id="anti-leakage"
                            title="界面隐私保护"
                            description="在投屏、录屏或公共场合使用时防止敏感信息意外露出。"
                            action={
                                savedPulse ? (
                                    <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
                                        <Check className="h-3.5 w-3.5 text-emerald-500" />
                                        已保存
                                    </span>
                                ) : null
                            }
                        >
                            <div className={settingRowClassName}>
                                <SettingRowText
                                    label="防泄露模式"
                                    description="开启后对话标题与敏感内容将被脱敏遮挡，保护屏幕隐私。"
                                />
                                <Switch
                                    aria-label="防泄露模式"
                                    checked={antiLeakageEnabled}
                                    disabled={antiLeakageBusy || loading}
                                    onCheckedChange={(checked) => void handleAntiLeakageChange(checked)}
                                />
                            </div>
                        </SettingsGroup>

                        <SettingsGroup
                            id="security-overview"
                            title="全局安全策略状态"
                            description="快速核对各安全子系统的当前配置并一键直达修改。"
                        >
                            <div className={settingRowClassName}>
                                <SettingRowText
                                    label="端到端凭据同步加密"
                                    description={
                                        syncSnapshot?.mode === "self_hosted"
                                            ? syncSnapshot.secret_sync_message || "已连接自建同步服务，请在数据与同步页管理同步口令。"
                                            : "当前为本地模式，凭据仅通过系统钥匙串加密保存在本机。"
                                    }
                                />
                                <SettingsLink
                                    menu="data-sync-config"
                                    subNav="#passphrase"
                                    navigateTo={navigateTo}
                                >
                                    管理同步口令
                                </SettingsLink>
                            </div>

                            <div className={settingRowClassName}>
                                <SettingRowText
                                    label="工具调用自动审核"
                                    description={
                                        effectiveReviewModel
                                            ? `已配置审核模型（${effectiveReviewModel.split("%%")[0]}），对开启自动审核的助手生效。`
                                            : "尚未配置自动审核模型，高危与外部工具调用均需人工确认。"
                                    }
                                />
                                <SettingsLink
                                    menu="ai-automation-config"
                                    subNav="#auto-review-model"
                                    navigateTo={navigateTo}
                                >
                                    配置审核模型
                                </SettingsLink>
                            </div>

                            <div className={settingRowClassName}>
                                <SettingRowText
                                    label="总管家工作区权限"
                                    description={
                                        !butlerEnabled
                                            ? "总管家模式未启用。"
                                            : trustAllWorkspaces
                                                ? "当前允许自动访问任意路径（高风险）。"
                                                : "仅自动放行主工作区与额外可信工作区，其余路径需确认。"
                                    }
                                />
                                <SettingsLink
                                    menu="butler-config"
                                    subNav="workspaces#butler-workspaces"
                                    navigateTo={navigateTo}
                                >
                                    查看工作区
                                </SettingsLink>
                            </div>
                        </SettingsGroup>
                    </div>
                </TabsContent>
            </Tabs>
        </div>
    );
};

export default SecurityVaultConfig;
