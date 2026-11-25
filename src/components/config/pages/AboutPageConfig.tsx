import React from "react";
import { useForm } from "react-hook-form";
import { AboutConfigForm } from "@/components/config/feature/forms/AboutConfigForm";
import OptionalConfigSection from "@/components/config/OptionalConfigSection";
import SettingsPageShell from "./SettingsPageShell";

export const AboutPageConfig: React.FC = () => {
    const form = useForm({ defaultValues: {} });

    return (
        <SettingsPageShell
            title="关于"
            description="查看 AIPP 版本信息、检查更新与实验特性说明。"
        >
            <AboutConfigForm form={form} />
            <OptionalConfigSection
                title="实验室"
                description="成熟功能（总管家、MCP 动态加载、上下文压缩、对话总结）均已归位至各自专属设置页。未来新增的高风险预览开关将临时放置在此。"
            >
                <p className="rounded-lg border border-dashed border-border px-4 py-3 text-xs text-muted-foreground">
                    当前版本暂无待启用的临时实验开关。
                </p>
            </OptionalConfigSection>
        </SettingsPageShell>
    );
};

export default AboutPageConfig;
