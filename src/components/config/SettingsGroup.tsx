import type { ReactNode } from "react";

/** 扁平设置列表：一组一个边框容器，行之间用分隔线。总管家设置与其子功能共用，保证视觉一致。 */
export const settingRowClassName = "flex items-center justify-between gap-4 px-4 py-3";
export const stackedRowClassName = "grid gap-2 px-4 py-3";

export function SettingsGroup({
    title,
    description,
    action,
    children,
}: {
    title: ReactNode;
    description?: ReactNode;
    action?: ReactNode;
    children: ReactNode;
}) {
    return (
        <section className="space-y-2">
            <div className="flex items-end justify-between gap-3">
                <div className="min-w-0">
                    <h3 className="text-sm font-medium">{title}</h3>
                    {description ? <p className="mt-0.5 text-xs text-muted-foreground">{description}</p> : null}
                </div>
                {action}
            </div>
            <div className="divide-y rounded-lg border">{children}</div>
        </section>
    );
}

export function SettingRowText({ label, description }: { label: ReactNode; description?: ReactNode }) {
    return (
        <div className="min-w-0 space-y-0.5">
            <div className="text-sm font-medium">{label}</div>
            {description ? <p className="text-xs text-muted-foreground">{description}</p> : null}
        </div>
    );
}
