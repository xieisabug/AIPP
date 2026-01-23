import React from "react";
import { UseFormReturn } from "react-hook-form";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Loader2, Download, CheckCircle2, Info, Globe } from "lucide-react";
import { useAppUpdater } from "@/hooks/useAppUpdater";

interface AboutConfigFormProps {
    form: UseFormReturn<any>;
}

export const AboutConfigForm: React.FC<AboutConfigFormProps> = ({ form: _form }) => {
    const {
        currentVersion,
        updateInfo,
        isChecking,
        isCheckingWithProxy,
        isDownloading,
        isDownloadingWithProxy,
        checkUpdate,
        checkUpdateWithProxy,
        downloadAndInstall,
        downloadAndInstallWithProxy,
    } = useAppUpdater();

    const getUpdateStatusBadge = () => {
        if (isChecking || isCheckingWithProxy) {
            return <Badge variant="outline"><Loader2 className="h-3 w-3 animate-spin mr-1" />检查中</Badge>;
        }
        if (updateInfo?.available) {
            return <Badge variant="destructive">有新版本 {updateInfo.latest_version}</Badge>;
        }
        if (updateInfo && !updateInfo.available) {
            return <Badge variant="outline" className="text-green-600"><CheckCircle2 className="h-3 w-3 mr-1" />已是最新</Badge>;
        }
        return null;
    };

    const handleUpdateButtonClick = () => {
        if (updateInfo?.available) {
            downloadAndInstall();
        } else {
            checkUpdate();
        }
    };

    const getUpdateButtonText = () => {
        if (isDownloading || isDownloadingWithProxy) return "下载中...";
        if (updateInfo?.available) return "开始更新";
        return "检查更新";
    };

    const isAnyChecking = isChecking || isCheckingWithProxy;
    const isAnyDownloading = isDownloading || isDownloadingWithProxy;
    const isAnyBusy = isAnyChecking || isAnyDownloading;

    return (
        <Card className="shadow-sm border-l-4 border-l-primary">
            <CardHeader>
                <CardTitle className="text-lg font-semibold">
                    关于 AIPP
                </CardTitle>
                <CardDescription>
                    查看应用信息和检查更新
                </CardDescription>
            </CardHeader>
            <CardContent className="space-y-6">
                {/* 当前版本 */}
                <div className="flex items-center justify-between p-4 bg-muted rounded-lg">
                    <div className="space-y-1">
                        <div className="text-sm text-muted-foreground">当前版本</div>
                        <div className="text-2xl font-bold">{currentVersion || "加载中..."}</div>
                    </div>
                    {getUpdateStatusBadge()}
                </div>

                {/* 更新说明 */}
                {updateInfo?.available && updateInfo.body && (
                    <div className="p-4 bg-muted rounded-lg">
                        <div className="flex items-center gap-2 mb-2">
                            <Info className="h-4 w-4 text-muted-foreground" />
                            <span className="text-sm font-medium">更新说明</span>
                        </div>
                        <div className="text-sm text-muted-foreground whitespace-pre-wrap">
                            {updateInfo.body}
                        </div>
                    </div>
                )}

                {/* 操作按钮 */}
                <div className="flex gap-3">
                    <Button
                        onClick={handleUpdateButtonClick}
                        disabled={isAnyBusy}
                        className="flex-1"
                        variant={updateInfo?.available ? "default" : "outline"}
                    >
                        {isAnyBusy && <Loader2 className="h-4 w-4 mr-2 animate-spin" />}
                        {!isAnyBusy && <Download className="h-4 w-4 mr-2" />}
                        {getUpdateButtonText()}
                    </Button>
                    {updateInfo?.available ? (
                        <Button
                            onClick={downloadAndInstallWithProxy}
                            disabled={isAnyBusy}
                            variant="outline"
                            className="shrink-0"
                            title="使用网络配置中的代理下载更新"
                        >
                            {isDownloadingWithProxy ? (
                                <Loader2 className="h-4 w-4 animate-spin" />
                            ) : (
                                <Globe className="h-4 w-4" />
                            )}
                        </Button>
                    ) : (
                        <Button
                            onClick={checkUpdateWithProxy}
                            disabled={isAnyBusy}
                            variant="outline"
                            className="shrink-0"
                            title="使用网络配置中的代理检查更新"
                        >
                            {isCheckingWithProxy ? <Loader2 className="h-4 w-4 animate-spin" /> : <Globe className="h-4 w-4" />}
                        </Button>
                    )}
                </div>
            </CardContent>
        </Card>
    );
};

export default AboutConfigForm;
