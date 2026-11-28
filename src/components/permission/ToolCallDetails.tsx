import type { ReactNode } from "react";
import { argumentLabel, isCodeArgument, type ToolCallView } from "./toolCallView";

const codeBlockClassName =
    "max-h-56 w-full max-w-full overflow-auto rounded-md bg-background px-2 py-1.5 font-mono text-xs leading-relaxed whitespace-pre-wrap break-all";

function Row({ label, children }: { label: string; children: ReactNode }) {
    return <div className="contents">
        <dt className="whitespace-nowrap pt-px text-muted-foreground">{label}</dt>
        <dd className="min-w-0 break-words text-foreground">{children}</dd>
    </div>;
}

function ArgumentValue({ name, value }: { name: string; value: unknown }) {
    if (typeof value === "boolean") return <>{value ? "是" : "否"}</>;
    if (typeof value === "number") return <>{value}</>;
    if (typeof value === "string") {
        return isCodeArgument(name, value)
            ? <pre className={codeBlockClassName}>{value}</pre>
            : <span className="whitespace-pre-wrap">{value}</span>;
    }
    return <pre className={codeBlockClassName}>{JSON.stringify(value, null, 2)}</pre>;
}

/** 审批卡的工具调用信息：工具在最上，然后是用途和逐项参数。 */
export function ToolCallDetails({ view, labels, children }: {
    view: ToolCallView;
    /** 工具自带的参数显示名，优先于内置译名。 */
    labels?: Map<string, string>;
    /** 附在参数后的补充信息，如折叠的原始请求。 */
    children?: ReactNode;
}) {
    const aside = [view.server, view.label ? view.name : undefined].filter(Boolean).join(" · ");
    return <div className="min-w-0 space-y-2 overflow-hidden rounded-md bg-muted p-3 text-sm">
        <dl className="grid grid-cols-[auto_minmax(0,1fr)] items-baseline gap-x-3 gap-y-2">
            <Row label="工具">
                <span className="font-medium">{view.label || view.name || "未提供工具名"}</span>
                {aside && <span className="ml-2 break-all text-xs text-muted-foreground">{aside}</span>}
            </Row>
            {view.purpose && <Row label="用途">{view.purpose}</Row>}
            {view.args.map(([key, value]) => <Row key={key} label={argumentLabel(key, labels)}>
                <ArgumentValue name={key} value={value} />
            </Row>)}
        </dl>
        {children}
    </div>;
}

export function RawRequestDetails({ summary = "请求详情", open, children }: { summary?: string; open?: boolean; children: ReactNode }) {
    return <details open={open} className="text-xs text-muted-foreground">
        <summary className="cursor-pointer select-none">{summary}</summary>
        <div className="mt-1 space-y-1">{children}</div>
    </details>;
}

export function RawText({ text }: { text: string }) {
    return <pre className={codeBlockClassName}>{text}</pre>;
}
