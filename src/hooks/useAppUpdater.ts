import { useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { UpdateInfo } from "@/data/Update";

export const useAppUpdater = () => {
    const [currentVersion, setCurrentVersion] = useState<string>("");
    const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
    const [isChecking, setIsChecking] = useState(false);
    const [isCheckingWithProxy, setIsCheckingWithProxy] = useState(false);
    const [isDownloading, setIsDownloading] = useState(false);

    // 获取当前版本
    useEffect(() => {
        invoke<string>("get_app_version").then(setCurrentVersion);
    }, []);

    // 检查更新
    const checkUpdate = useCallback(async () => {
        setIsChecking(true);
        try {
            const info = await invoke<UpdateInfo>("check_update");
            setUpdateInfo(info);
            if (info.available) {
                toast.success(`发现新版本: ${info.latest_version}`);
            } else {
                toast.info("当前已是最新版本");
            }
        } catch (e) {
            toast.error("检查更新失败: " + e);
        } finally {
            setIsChecking(false);
        }
    }, []);

    // 使用代理检查更新
    const checkUpdateWithProxy = useCallback(async () => {
        setIsCheckingWithProxy(true);
        try {
            const info = await invoke<UpdateInfo>("check_update_with_proxy");
            setUpdateInfo(info);
            if (info.available) {
                toast.success(`发现新版本: ${info.latest_version}`);
            } else {
                toast.info("当前已是最新版本");
            }
        } catch (e) {
            toast.error("代理检查更新失败: " + e);
        } finally {
            setIsCheckingWithProxy(false);
        }
    }, []);

    // 下载并安装更新
    const downloadAndInstall = useCallback(async () => {
        setIsDownloading(true);
        try {
            const msg = await invoke<string>("download_and_install_update");
            toast.success(msg);
        } catch (e) {
            toast.error("更新失败: " + e);
            setIsDownloading(false);
        }
    }, []);

    return {
        currentVersion,
        updateInfo,
        isChecking,
        isCheckingWithProxy,
        isDownloading,
        checkUpdate,
        checkUpdateWithProxy,
        downloadAndInstall,
    };
};
