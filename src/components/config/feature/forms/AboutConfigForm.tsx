import { useEffect, useState } from "react";
import type { UseFormReturn } from "react-hook-form";
import { invoke } from "@tauri-apps/api/core";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { AppUpdateDetails } from "@/components/AppUpdateNotice";
import { useAppUpdater } from "@/hooks/useAppUpdater";

export function AboutConfigForm(_props: { form: UseFormReturn<any> }) {
    const { available, status, readError, canRestart } = useAppUpdater();
    const [version, setVersion] = useState("加载中…");
    useEffect(() => {
        void invoke<string>("get_app_version").then(setVersion).catch(error => setVersion(`读取版本失败：${error}`));
    }, []);

    return <Card className="shadow-none border-border" data-aipp-config-section>
        <CardHeader>
            <CardTitle className="text-lg font-semibold">关于 AIPP</CardTitle>
            <CardDescription>查看应用信息和检查更新</CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
            <div className="space-y-1 rounded-lg bg-muted p-4">
                <div className="text-sm text-muted-foreground">当前版本</div>
                <div className="text-2xl font-bold">{version}</div>
            </div>
            {available && readError && <p role="alert" className="text-sm text-destructive">{readError}</p>}
            {available && status && <AppUpdateDetails status={status} canRestart={canRestart} />}
        </CardContent>
    </Card>;
}

export default AboutConfigForm;
