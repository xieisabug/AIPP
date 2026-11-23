import { emit } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import { Switch } from "../ui/switch";
import { Button } from "../ui/button";

export function useProviderProxyConfigured() {
    const { getConfigValue, loading } = useFeatureConfig();
    return loading ? null : Boolean(getConfigValue("network_config", "network_proxy").trim());
}

interface ProviderProxySwitchProps {
    id: string;
    description: string;
    configured: boolean | null;
    checked: boolean;
    onCheckedChange: (checked: boolean) => void;
    busy?: boolean;
    onLeaveWizard?: () => void;
}

export function ProviderProxySwitch({
    id, description, configured, checked, onCheckedChange, busy, onLeaveWizard,
}: ProviderProxySwitchProps) {
    async function openNetworkSettings() {
        try {
            await emit("config-navigate-to", { menu: "feature-assistant-config", subNav: "network_config" });
            onLeaveWizard?.();
        } catch (error) {
            toast.error(`打开网络配置失败：${String(error)}`);
        }
    }

    return (
        <div className="flex items-center justify-between gap-4">
            <div className="flex min-w-0 flex-col">
                <label htmlFor={id} className="text-sm font-medium text-foreground">使用网络代理进行请求</label>
                <span id={`${id}-hint`} className="text-xs text-muted-foreground">
                    {configured === false ? (
                        <>
                            尚未配置全局网络代理。
                            <Button type="button" variant="link" className="h-auto p-0 text-xs underline"
                                disabled={busy} onClick={openNetworkSettings}>
                                {onLeaveWizard ? "退出向导并配置代理" : "前往配置代理"}
                            </Button>
                        </>
                    ) : description}
                </span>
            </div>
            <Switch id={id} aria-describedby={`${id}-hint`} disabled={!configured || busy}
                checked={configured === true && checked} onCheckedChange={onCheckedChange} />
        </div>
    );
}
