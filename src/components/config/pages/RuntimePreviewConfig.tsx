import React, { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronDown, Code2, Plus, RefreshCw, Table2, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ConfigSaveFooter } from "@/components/common/ConfigSaveFooter";
import { SettingsGroup, SettingRowText, settingRowClassName, stackedRowClassName } from "@/components/config/SettingsGroup";
import { useVersionManager } from "@/hooks/feature/useVersionManager";
import { useUnsavedLeaveGuard } from "@/hooks/useUnsavedLeaveGuard";
import { parseSubNav, useSettingsAnchorHighlight } from "@/lib/settingsNavigation";
import { getErrorMessage } from "@/utils/error";
import SettingsPageShell from "./SettingsPageShell";

interface PreviewResourceDomainRule {
    domain: string;
    includeSubdomains: boolean;
    types: string[];
    autoLoad: boolean;
}

interface PreviewExternalResourcePolicy {
    allowedDomains: PreviewResourceDomainRule[];
    limits: {
        maxResourceBytes: number;
        requestTimeoutSecs: number;
        cacheTtlSecs: number;
    };
}

const RESOURCE_TYPE_OPTIONS = [
    { id: "script", label: "脚本 (script)" },
    { id: "css", label: "样式 (css)" },
    { id: "image", label: "图片 (image)" },
    { id: "font", label: "字体 (font)" },
] as const;

const DEFAULT_POLICY: PreviewExternalResourcePolicy = {
    allowedDomains: [],
    limits: {
        maxResourceBytes: 5 * 1024 * 1024,
        requestTimeoutSecs: 15,
        cacheTtlSecs: 3600,
    },
};

function normalizePolicy(raw: any): PreviewExternalResourcePolicy {
    const allowedDomains: PreviewResourceDomainRule[] = Array.isArray(raw?.allowedDomains)
        ? raw.allowedDomains.map((r: any) => ({
            domain: String(r?.domain ?? ""),
            includeSubdomains: Boolean(r?.includeSubdomains),
            types: Array.isArray(r?.types) ? r.types.map(String) : [],
            autoLoad: Boolean(r?.autoLoad),
        }))
        : [];
    return {
        allowedDomains,
        limits: {
            maxResourceBytes: Number(raw?.limits?.maxResourceBytes ?? DEFAULT_POLICY.limits.maxResourceBytes),
            requestTimeoutSecs: Number(raw?.limits?.requestTimeoutSecs ?? DEFAULT_POLICY.limits.requestTimeoutSecs),
            cacheTtlSecs: Number(raw?.limits?.cacheTtlSecs ?? DEFAULT_POLICY.limits.cacheTtlSecs),
        },
    };
}

interface RuntimePreviewConfigProps {
    subNav?: string;
    onSubNavConsumed?: () => void;
}

export const RuntimePreviewConfig: React.FC<RuntimePreviewConfigProps> = ({
    subNav,
    onSubNavConsumed,
}) => {
    const vm = useVersionManager();
    const [policy, setPolicy] = useState<PreviewExternalResourcePolicy>(DEFAULT_POLICY);
    const [jsonMode, setJsonMode] = useState(false);
    const [jsonText, setJsonText] = useState("");
    const [policyLoaded, setPolicyLoaded] = useState(false);
    const [policyError, setPolicyError] = useState("");
    const [savingPolicy, setSavingPolicy] = useState(false);
    const [newDomain, setNewDomain] = useState("");
    const [anchor, setAnchor] = useState<string | null>(null);
    const baselineJsonRef = useRef("");

    useEffect(() => {
        if (!subNav) return;
        const parsed = parseSubNav(subNav);
        if (parsed.anchor) setAnchor(parsed.anchor);
        onSubNavConsumed?.();
    }, [subNav, onSubNavConsumed]);

    useSettingsAnchorHighlight(anchor, () => setAnchor(null), String(policyLoaded));

    useEffect(() => {
        let active = true;
        invoke<PreviewExternalResourcePolicy>("get_preview_external_resource_policy")
            .then((loaded) => {
                if (!active) return;
                const normalized = normalizePolicy(loaded);
                const formatted = JSON.stringify(normalized, null, 2);
                setPolicy(normalized);
                setJsonText(formatted);
                baselineJsonRef.current = formatted;
                setPolicyError("");
                setPolicyLoaded(true);
            })
            .catch((err) => {
                if (!active) return;
                setPolicyError(getErrorMessage(err));
                setPolicyLoaded(true);
            });
        return () => { active = false; };
    }, []);

    const currentSerialized = jsonMode ? jsonText : JSON.stringify(policy, null, 2);
    const isDirty = policyLoaded && currentSerialized !== baselineJsonRef.current;

    const handleToggleJsonMode = (toJson: boolean) => {
        if (toJson) {
            setJsonText(JSON.stringify(policy, null, 2));
            setJsonMode(true);
            return;
        }
        try {
            const parsed = normalizePolicy(JSON.parse(jsonText));
            setPolicy(parsed);
            setPolicyError("");
            setJsonMode(false);
        } catch (err) {
            toast.error("JSON 格式有误，请修正后再切回可视化编辑：" + getErrorMessage(err));
        }
    };

    const handleAddDomain = () => {
        const trimmed = newDomain.trim().replace(/^https?:\/\//i, "").replace(/\/.*$/, "");
        if (!trimmed) return;
        if (policy.allowedDomains.some((d) => d.domain.toLowerCase() === trimmed.toLowerCase())) {
            toast.error("该域名已在白名单中");
            return;
        }
        setPolicy((prev) => ({
            ...prev,
            allowedDomains: [
                ...prev.allowedDomains,
                { domain: trimmed, includeSubdomains: false, types: ["script", "css"], autoLoad: true },
            ],
        }));
        setNewDomain("");
    };

    const handleSavePolicy = useCallback(async () => {
        setSavingPolicy(true);
        setPolicyError("");
        try {
            const payload = jsonMode ? normalizePolicy(JSON.parse(jsonText)) : policy;
            await invoke("save_preview_external_resource_policy", { policy: payload });
            const formatted = JSON.stringify(payload, null, 2);
            setPolicy(payload);
            setJsonText(formatted);
            baselineJsonRef.current = formatted;
            toast.success("预览白名单策略已保存");
        } catch (err) {
            const msg = getErrorMessage(err);
            setPolicyError(msg);
            toast.error("保存白名单策略失败：" + msg);
            throw err;
        } finally {
            setSavingPolicy(false);
        }
    }, [jsonMode, jsonText, policy]);

    const handleDiscardPolicy = useCallback(() => {
        if (!baselineJsonRef.current) return;
        try {
            const restored = normalizePolicy(JSON.parse(baselineJsonRef.current));
            setPolicy(restored);
            setJsonText(baselineJsonRef.current);
            setPolicyError("");
        } catch {
            // ignore
        }
    }, []);

    const { dialog: leaveDialog } = useUnsavedLeaveGuard({
        isDirty,
        onSave: handleSavePolicy,
        onDiscard: handleDiscardPolicy,
    });

    const bunInstalled = vm.bunVersion && vm.bunVersion !== "Not Installed";
    const uvInstalled = vm.uvVersion && vm.uvVersion !== "Not Installed";

    return (
        <>
            <SettingsPageShell
                title="运行环境与预览"
                description="管理用于 Artifact 构建、脚本运行的本地运行时，以及 HTML/React/Vue 预览的外部资源安全白名单。"
                headerExtra={
                    isDirty ? (
                        <span className="rounded-full border border-foreground/30 bg-muted px-2.5 py-0.5 text-xs font-medium">
                            白名单未保存
                        </span>
                    ) : null
                }
            >
                <SettingsGroup
                    id="runtimes"
                    title="本地运行时"
                    description="安装或更新操作立即执行，不影响下方的白名单策略表单。"
                >
                    {/* Bun */}
                    <div className={stackedRowClassName}>
                        <div className="flex flex-wrap items-center justify-between gap-3">
                            <SettingRowText
                                label={
                                    <span className="inline-flex items-center gap-2">
                                        Bun
                                        <span className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] font-normal text-muted-foreground">
                                            {vm.bunVersion || "检测中…"}
                                        </span>
                                        {vm.bunLatestVersion && bunInstalled ? (
                                            <span className="rounded bg-primary/10 px-1.5 py-0.5 text-[11px] font-normal text-primary">
                                                可更新至 {vm.bunLatestVersion}
                                            </span>
                                        ) : null}
                                    </span>
                                }
                                description="用于 React / Vue Artifact 预览与前端项目临时构建。"
                            />
                            <div className="flex shrink-0 items-center gap-1.5">
                                {!bunInstalled ? (
                                    <Button
                                        type="button"
                                        size="sm"
                                        disabled={vm.isInstallingBun}
                                        onClick={vm.installBun}
                                    >
                                        {vm.isInstallingBun ? "安装中…" : "安装 Bun"}
                                    </Button>
                                ) : (
                                    <>
                                        {vm.bunLatestVersion ? (
                                            <Button
                                                type="button"
                                                size="sm"
                                                disabled={vm.isUpdatingBun}
                                                onClick={() => vm.updateBun(false)}
                                            >
                                                {vm.isUpdatingBun ? "更新中…" : `更新到 ${vm.bunLatestVersion}`}
                                            </Button>
                                        ) : (
                                            <Button
                                                type="button"
                                                size="sm"
                                                variant="outline"
                                                disabled={vm.isCheckingBunUpdate || vm.isUpdatingBun}
                                                onClick={() => vm.checkBunUpdate(false)}
                                            >
                                                {vm.isCheckingBunUpdate ? "检查中…" : "检查更新"}
                                            </Button>
                                        )}
                                        <DropdownMenu>
                                            <DropdownMenuTrigger asChild>
                                                <Button type="button" size="sm" variant="outline" aria-label="Bun 更多操作">
                                                    <ChevronDown className="h-3.5 w-3.5" />
                                                </Button>
                                            </DropdownMenuTrigger>
                                            <DropdownMenuContent align="end">
                                                <DropdownMenuItem
                                                    disabled={vm.isCheckingBunUpdate || vm.isUpdatingBun}
                                                    onSelect={() => vm.checkBunUpdate(true)}
                                                >
                                                    使用代理检查更新
                                                </DropdownMenuItem>
                                                {vm.bunLatestVersion ? (
                                                    <DropdownMenuItem
                                                        disabled={vm.isUpdatingBun}
                                                        onSelect={() => vm.updateBun(true)}
                                                    >
                                                        使用代理更新到 {vm.bunLatestVersion}
                                                    </DropdownMenuItem>
                                                ) : null}
                                            </DropdownMenuContent>
                                        </DropdownMenu>
                                    </>
                                )}
                            </div>
                        </div>
                        {(vm.isInstallingBun || vm.isUpdatingBun) && vm.bunInstallLog ? (
                            <pre className="max-h-36 overflow-auto rounded-md bg-muted p-2.5 font-mono text-[11px] whitespace-pre-wrap">
                                {vm.bunInstallLog}
                            </pre>
                        ) : null}
                    </div>

                    {/* UV */}
                    <div className={stackedRowClassName}>
                        <div className="flex flex-wrap items-center justify-between gap-3">
                            <SettingRowText
                                label={
                                    <span className="inline-flex items-center gap-2">
                                        uv
                                        <span className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] font-normal text-muted-foreground">
                                            {vm.uvVersion || "检测中…"}
                                        </span>
                                        {vm.uvLatestVersion && uvInstalled ? (
                                            <span className="rounded bg-primary/10 px-1.5 py-0.5 text-[11px] font-normal text-primary">
                                                可更新至 {vm.uvLatestVersion}
                                            </span>
                                        ) : null}
                                    </span>
                                }
                                description="用于管理 Python 运行环境与运行 Python 代码块。"
                            />
                            <div className="flex shrink-0 items-center gap-1.5">
                                {!uvInstalled ? (
                                    <Button
                                        type="button"
                                        size="sm"
                                        disabled={vm.isInstallingUv}
                                        onClick={vm.installUv}
                                    >
                                        {vm.isInstallingUv ? "安装中…" : "安装 uv"}
                                    </Button>
                                ) : (
                                    <>
                                        {vm.uvLatestVersion ? (
                                            <Button
                                                type="button"
                                                size="sm"
                                                disabled={vm.isUpdatingUv}
                                                onClick={() => vm.updateUv(false)}
                                            >
                                                {vm.isUpdatingUv ? "更新中…" : `更新到 ${vm.uvLatestVersion}`}
                                            </Button>
                                        ) : (
                                            <Button
                                                type="button"
                                                size="sm"
                                                variant="outline"
                                                disabled={vm.isCheckingUvUpdate || vm.isUpdatingUv}
                                                onClick={() => vm.checkUvUpdate(false)}
                                            >
                                                {vm.isCheckingUvUpdate ? "检查中…" : "检查更新"}
                                            </Button>
                                        )}
                                        <DropdownMenu>
                                            <DropdownMenuTrigger asChild>
                                                <Button type="button" size="sm" variant="outline" aria-label="uv 更多操作">
                                                    <ChevronDown className="h-3.5 w-3.5" />
                                                </Button>
                                            </DropdownMenuTrigger>
                                            <DropdownMenuContent align="end">
                                                <DropdownMenuItem
                                                    disabled={vm.isCheckingUvUpdate || vm.isUpdatingUv}
                                                    onSelect={() => vm.checkUvUpdate(true)}
                                                >
                                                    使用代理检查更新
                                                </DropdownMenuItem>
                                                {vm.uvLatestVersion ? (
                                                    <DropdownMenuItem
                                                        disabled={vm.isUpdatingUv}
                                                        onSelect={() => vm.updateUv(true)}
                                                    >
                                                        使用代理更新到 {vm.uvLatestVersion}
                                                    </DropdownMenuItem>
                                                ) : null}
                                            </DropdownMenuContent>
                                        </DropdownMenu>
                                    </>
                                )}
                            </div>
                        </div>
                        {(vm.isInstallingUv || vm.isUpdatingUv) && vm.uvInstallLog ? (
                            <pre className="max-h-36 overflow-auto rounded-md bg-muted p-2.5 font-mono text-[11px] whitespace-pre-wrap">
                                {vm.uvInstallLog}
                            </pre>
                        ) : null}
                    </div>

                    {/* Python */}
                    {uvInstalled ? (
                        <div className={stackedRowClassName}>
                            <div className="flex flex-wrap items-center justify-between gap-3">
                                <SettingRowText
                                    label={
                                        <span className="inline-flex items-center gap-2">
                                            Python
                                            <span className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] font-normal text-muted-foreground">
                                                Python 3: {vm.python3Version || "检测中…"}
                                            </span>
                                        </span>
                                    }
                                    description={
                                        vm.installedPythons.length > 0
                                            ? `uv 已托管版本：${vm.installedPythons.join(", ")}`
                                            : vm.needInstallPython3
                                                ? "未检测到可用 Python 3，建议通过 uv 安装。"
                                                : `Python 2: ${vm.python2Version || "未安装"}`
                                    }
                                />
                                <div className="flex shrink-0 items-center gap-1.5">
                                    {vm.needInstallPython3 ? (
                                        <Button
                                            type="button"
                                            size="sm"
                                            disabled={vm.isInstallingPython}
                                            onClick={vm.installPython3}
                                        >
                                            {vm.isInstallingPython ? "安装中…" : "安装 Python 3"}
                                        </Button>
                                    ) : null}
                                    <Button
                                        type="button"
                                        size="sm"
                                        variant="outline"
                                        disabled={vm.isInstallingPython}
                                        onClick={vm.checkPythonVersions}
                                    >
                                        <RefreshCw className="mr-1 h-3.5 w-3.5" />
                                        刷新
                                    </Button>
                                </div>
                            </div>
                            {vm.isInstallingPython && vm.pythonInstallLog ? (
                                <pre className="max-h-36 overflow-auto rounded-md bg-muted p-2.5 font-mono text-[11px] whitespace-pre-wrap">
                                    {vm.pythonInstallLog}
                                </pre>
                            ) : null}
                        </div>
                    ) : null}
                </SettingsGroup>

                <SettingsGroup
                    id="preview-whitelist"
                    title="预览外部资源白名单策略"
                    description="控制代码预览沙箱中允许加载的外部 CDN 域名、资源类型和网络限额。"
                    action={
                        <Button
                            type="button"
                            variant="outline"
                            size="sm"
                            onClick={() => handleToggleJsonMode(!jsonMode)}
                        >
                            {jsonMode ? (
                                <>
                                    <Table2 className="mr-1.5 h-3.5 w-3.5" />
                                    可视化编辑
                                </>
                            ) : (
                                <>
                                    <Code2 className="mr-1.5 h-3.5 w-3.5" />
                                    编辑 JSON
                                </>
                            )}
                        </Button>
                    }
                >
                    {jsonMode ? (
                        <div className={stackedRowClassName}>
                            <Textarea
                                value={jsonText}
                                onChange={(e) => setJsonText(e.target.value)}
                                className="min-h-60 font-mono text-xs"
                                spellCheck={false}
                                aria-label="外部资源白名单策略 JSON"
                            />
                            {policyError ? <p className="text-xs text-destructive">{policyError}</p> : null}
                        </div>
                    ) : (
                        <>
                            <div className={stackedRowClassName}>
                                <SettingRowText
                                    label="允许的外部域名"
                                    description="添加如 cdn.jsdelivr.net 或 unpkg.com，并设置允许的资源类型与是否自动加载。"
                                />
                                <div className="flex items-center gap-2">
                                    <Input
                                        value={newDomain}
                                        onChange={(e) => setNewDomain(e.target.value)}
                                        onKeyDown={(e) => {
                                            if (e.key === "Enter") {
                                                e.preventDefault();
                                                handleAddDomain();
                                            }
                                        }}
                                        placeholder="输入域名，例如 cdn.jsdelivr.net"
                                    />
                                    <Button
                                        type="button"
                                        variant="outline"
                                        size="sm"
                                        disabled={!newDomain.trim()}
                                        onClick={handleAddDomain}
                                    >
                                        <Plus className="mr-1 h-3.5 w-3.5" />
                                        添加域名
                                    </Button>
                                </div>
                                {policy.allowedDomains.length === 0 ? (
                                    <p className="py-2 text-xs text-muted-foreground">暂无白名单域名规则</p>
                                ) : (
                                    <div className="divide-y rounded-md border">
                                        {policy.allowedDomains.map((rule, index) => (
                                            <div key={`${rule.domain}-${index}`} className="space-y-2.5 p-3">
                                                <div className="flex flex-wrap items-center justify-between gap-2">
                                                    <span className="font-mono text-xs font-medium">{rule.domain}</span>
                                                    <div className="flex items-center gap-4">
                                                        <label className="flex cursor-pointer items-center gap-1.5 text-xs text-muted-foreground">
                                                            <Switch
                                                                checked={rule.includeSubdomains}
                                                                onCheckedChange={(checked) =>
                                                                    setPolicy((prev) => ({
                                                                        ...prev,
                                                                        allowedDomains: prev.allowedDomains.map((item, i) =>
                                                                            i === index ? { ...item, includeSubdomains: checked } : item
                                                                        ),
                                                                    }))
                                                                }
                                                            />
                                                            含子域名
                                                        </label>
                                                        <label className="flex cursor-pointer items-center gap-1.5 text-xs text-muted-foreground">
                                                            <Switch
                                                                checked={rule.autoLoad}
                                                                onCheckedChange={(checked) =>
                                                                    setPolicy((prev) => ({
                                                                        ...prev,
                                                                        allowedDomains: prev.allowedDomains.map((item, i) =>
                                                                            i === index ? { ...item, autoLoad: checked } : item
                                                                        ),
                                                                    }))
                                                                }
                                                            />
                                                            自动加载
                                                        </label>
                                                        <Button
                                                            type="button"
                                                            variant="ghost"
                                                            size="icon"
                                                            aria-label={`删除 ${rule.domain}`}
                                                            onClick={() =>
                                                                setPolicy((prev) => ({
                                                                    ...prev,
                                                                    allowedDomains: prev.allowedDomains.filter((_, i) => i !== index),
                                                                }))
                                                            }
                                                            className="h-7 w-7 text-muted-foreground hover:text-destructive"
                                                        >
                                                            <Trash2 className="h-3.5 w-3.5" />
                                                        </Button>
                                                    </div>
                                                </div>
                                                <div className="flex flex-wrap gap-4">
                                                    {RESOURCE_TYPE_OPTIONS.map((t) => {
                                                        const checked = rule.types.includes(t.id);
                                                        return (
                                                            <label key={t.id} className="flex cursor-pointer items-center gap-1.5 text-xs">
                                                                <Checkbox
                                                                    checked={checked}
                                                                    onCheckedChange={(next) => {
                                                                        const enabled = next === true;
                                                                        setPolicy((prev) => ({
                                                                            ...prev,
                                                                            allowedDomains: prev.allowedDomains.map((item, i) => {
                                                                                if (i !== index) return item;
                                                                                const nextTypes = enabled
                                                                                    ? Array.from(new Set([...item.types, t.id]))
                                                                                    : item.types.filter((x) => x !== t.id);
                                                                                return { ...item, types: nextTypes };
                                                                            }),
                                                                        }));
                                                                    }}
                                                                />
                                                                {t.label}
                                                            </label>
                                                        );
                                                    })}
                                                </div>
                                            </div>
                                        ))}
                                    </div>
                                )}
                            </div>

                            <div className={settingRowClassName}>
                                <SettingRowText label="单资源体积上限（MB）" description="超出大小限制的外部资源将被拦截。" />
                                <Input
                                    type="number"
                                    min={1}
                                    max={100}
                                    className="w-28 shrink-0"
                                    value={Math.max(1, Math.round(policy.limits.maxResourceBytes / (1024 * 1024)))}
                                    onChange={(e) => {
                                        const mb = Math.max(1, Number(e.target.value) || 1);
                                        setPolicy((prev) => ({
                                            ...prev,
                                            limits: { ...prev.limits, maxResourceBytes: mb * 1024 * 1024 },
                                        }));
                                    }}
                                />
                            </div>
                            <div className={settingRowClassName}>
                                <SettingRowText label="外部资源请求超时（秒）" />
                                <Input
                                    type="number"
                                    min={3}
                                    max={120}
                                    className="w-28 shrink-0"
                                    value={policy.limits.requestTimeoutSecs}
                                    onChange={(e) =>
                                        setPolicy((prev) => ({
                                            ...prev,
                                            limits: { ...prev.limits, requestTimeoutSecs: Math.max(3, Number(e.target.value) || 15) },
                                        }))
                                    }
                                />
                            </div>
                            <div className={settingRowClassName}>
                                <SettingRowText label="本地缓存有效期（秒）" />
                                <Input
                                    type="number"
                                    min={0}
                                    className="w-28 shrink-0"
                                    value={policy.limits.cacheTtlSecs}
                                    onChange={(e) =>
                                        setPolicy((prev) => ({
                                            ...prev,
                                            limits: { ...prev.limits, cacheTtlSecs: Math.max(0, Number(e.target.value) || 0) },
                                        }))
                                    }
                                />
                            </div>
                        </>
                    )}
                </SettingsGroup>

                <ConfigSaveFooter>
                    <div className="flex items-center justify-between gap-3">
                        <div className="flex items-center gap-2">
                            <Button
                                type="button"
                                disabled={savingPolicy || !isDirty}
                                onClick={() => void handleSavePolicy()}
                            >
                                {savingPolicy ? "保存中…" : "保存白名单策略"}
                            </Button>
                            {isDirty ? (
                                <Button
                                    type="button"
                                    variant="outline"
                                    disabled={savingPolicy}
                                    onClick={handleDiscardPolicy}
                                >
                                    放弃更改
                                </Button>
                            ) : null}
                        </div>
                        {isDirty ? (
                            <span className="text-xs text-muted-foreground">白名单策略有未保存的更改</span>
                        ) : null}
                    </div>
                </ConfigSaveFooter>
            </SettingsPageShell>
            {leaveDialog}
        </>
    );
};

export default RuntimePreviewConfig;
