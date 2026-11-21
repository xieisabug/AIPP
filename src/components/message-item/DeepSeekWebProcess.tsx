import { emit } from "@tauri-apps/api/event";
import { WebToolCard, type WebToolMeta } from "../mcp-tool-components/WebToolCard";
import { deepSeekWebContextId, type WebCall, type WebReport } from "@/utils/deepSeekWeb";

function hostOf(url?: string) {
    try { return url ? new URL(url).hostname : undefined; } catch { return undefined; }
}

function metaOf(call: WebCall): WebToolMeta[] {
    if (call.failed || call.running) return [];
    if (call.type === "search") {
        return [
            { key: "engine", value: "DeepSeek", title: "由 DeepSeek 服务端搜索" },
            { key: "count", value: `${call.sources.length} 条`, title: `结果数量：${call.sources.length} 条` },
        ];
    }
    const host = call.type === "read" ? hostOf(call.url) : undefined;
    return host ? [{ key: "host", value: host, title: call.url ?? host }] : [];
}

/**
 * DeepSeek 服务端联网的每次调用画成与 AIPP 内置 search_web / fetch_url 相同的卡片，
 * 点击在侧栏中定位这次调用带回的来源。服务端调用在正文之前完成，所以卡片排在正文前面。
 */
export function DeepSeekWebProcess({ report, messageId, conversationId, settled }: {
    report: WebReport;
    messageId: number;
    conversationId: number;
    /** 回复已结束（完成、停止或出错）：仍未完成的调用按失败显示，不再转圈。 */
    settled: boolean;
}) {
    return (
        <div data-aipp-slot="deepseek-web-calls">
            {report.calls.map((call) => {
                const live = call.running && !settled;
                return <WebToolCard
                    key={call.key}
                    kind={call.type}
                    title={call.title}
                    titleTooltip={call.detail}
                    state={live ? "executing" : call.failed || call.running ? "failed" : "success"}
                    shine={live}
                    animateEntrance={live}
                    meta={metaOf(call)}
                    onOpen={call.sources.length > 0
                        ? () => void emit("sidebar-focus-context", { id: deepSeekWebContextId(messageId, call), conversationId })
                        : undefined}
                    openLabel="在侧栏中查看来源"
                />;
            })}
        </div>
    );
}
