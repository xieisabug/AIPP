import { useState } from "react";
import { MotionDetails, MotionToolCard } from "../mcp-tool-components/McpToolMotion";
import type { WebReport } from "@/utils/deepSeekWeb";

export function DeepSeekWebProcess({ report }: { report: WebReport }) {
    const [open, setOpen] = useState(false);
    return <MotionToolCard animateEntrance={false}>
        <button type="button" className="w-full text-left text-xs text-muted-foreground" aria-expanded={open} onClick={() => setOpen(!open)}>
            搜索了 {report.searchCount} 次 · 阅读 {report.pageCount} 个网页 · {report.sources.length} 个来源
        </button>
        <MotionDetails show={open}><ol className="mt-2 space-y-1 text-xs">
            {report.steps.map((step, i) => <li key={i} className="break-all">{step.type === "search" ? "搜索" : step.type === "find" ? "页内查找" : "阅读"} · {step.label}</li>)}
        </ol></MotionDetails>
    </MotionToolCard>;
}
