import React, { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { DataFolderConfigForm } from "@/components/config/feature/forms/DataFolderConfigForm";
import { parseSubNav, useSettingsAnchorHighlight } from "@/lib/settingsNavigation";
import SettingsPageShell from "./SettingsPageShell";

interface DataSyncConfigProps {
    subNav?: string;
    onSubNavConsumed?: () => void;
}

export const DataSyncConfig: React.FC<DataSyncConfigProps> = ({
    subNav,
    onSubNavConsumed,
}) => {
    const form = useForm({
        defaultValues: {
            mode: "local",
            server_url: "",
            token: "",
        },
    });
    const [anchor, setAnchor] = useState<string | null>(null);

    useEffect(() => {
        if (!subNav) return;
        const parsed = parseSubNav(subNav);
        if (parsed.anchor) setAnchor(parsed.anchor);
        onSubNavConsumed?.();
    }, [subNav, onSubNavConsumed]);

    useSettingsAnchorHighlight(anchor, () => setAnchor(null));

    return (
        <SettingsPageShell
            title="数据与同步"
            description="管理本机 SQLite 数据目录、自建跨设备同步服务与端到端凭据加密口令。"
        >
            <DataFolderConfigForm form={form} enableLeaveGuard />
        </SettingsPageShell>
    );
};

export default DataSyncConfig;
