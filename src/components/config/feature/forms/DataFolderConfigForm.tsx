import React, { useCallback } from "react";
import { UseFormReturn } from "react-hook-form";
import { invoke } from "@tauri-apps/api/core";
import ConfigForm from "@/components/ConfigForm";
import { toast } from "sonner";
import { getErrorMessage } from "@/utils/error";

interface DataFolderConfigFormProps {
    form: UseFormReturn<any>;
}

export const DataFolderConfigForm: React.FC<DataFolderConfigFormProps> = ({ form }) => {
    const handleOpenDataFolder = useCallback(async () => {
        try {
            await invoke("open_data_folder");
        } catch (error) {
            toast.error("打开数据目录失败: " + getErrorMessage(error));
        }
    }, []);

    const dataFolderConfig = [
        {
            key: "openDataFolder",
            config: {
                type: "button" as const,
                label: "数据文件夹",
                value: "打开",
                onClick: handleOpenDataFolder,
            },
        },
        {
            key: "localMode",
            config: {
                type: "static" as const,
                label: "当前模式",
                value: "当前版本仅使用本地 SQLite 数据库，不再提供 libsql 多端同步配置。",
            },
        },
    ];

    return (
        <ConfigForm
            title="数据目录"
            description="管理本地数据目录。当前版本默认使用本地 SQLite 数据库。"
            config={dataFolderConfig}
            layout="default"
            classNames="bottom-space"
            useFormReturn={form}
        />
    );
};

export default React.memo(DataFolderConfigForm);
