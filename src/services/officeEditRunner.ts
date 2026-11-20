/**
 * office_edit 的执行端：Rust 广播 `office-edit-request`，任一窗口认领后在隐藏的查看器 iframe 里
 * 按步骤修改文档、保存并把字节交回 Rust（Rust 校验后写副本）。多个窗口同时收到时只有一个能认领。
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
    encodeFramedRequest,
    parseOfficeFileResponse,
    resolveOfficeViewerUrl,
    viewerTargetOrigin,
} from "@/utils/officeFile";
import {
    getOfficePreviewPluginStatus,
    installOfficePreviewPlugin,
    isOfficeEditPluginReady,
} from "@/services/officePreviewPlugin";

export const OFFICE_EDIT_EVENT = "office-edit-request";
const VIEWER_READY_TIMEOUT_MS = 30_000;
const JOB_TIMEOUT_MS = 240_000;
const EDIT_PROTOCOL = 2;

interface EditRequestEvent {
    jobId: string;
    conversationId?: number | null;
    fileName: string;
    format: string;
}

interface ClaimedJob {
    jobId: string;
    format: "docx" | "xlsx" | "pptx";
    fileName: string;
    steps: unknown[];
}

export class OfficeJobError extends Error {
    readonly code: string;
    constructor(code: string, message: string) {
        super(message);
        this.code = code;
    }
}

async function ensureEditPlugin(): Promise<void> {
    const status = await getOfficePreviewPluginStatus();
    if (status.installed && !status.enabled && !status.updateRequired) {
        throw new OfficeJobError("plugin_disabled", "Office 组件已在插件中心停用，启用后才能修改 Office 文件");
    }
    if (isOfficeEditPluginReady(status)) return;
    try {
        const installed = await installOfficePreviewPlugin({ requireEdit: true });
        if (!isOfficeEditPluginReady(installed)) {
            throw new Error(`安装后组件仍不可用（版本 ${installed.version ?? "未知"}）`);
        }
    } catch (err) {
        throw new OfficeJobError("plugin_unavailable", `Office 组件安装或更新失败：${err instanceof Error ? err.message : String(err)}`);
    }
}

/** 在隐藏 iframe 里执行一次修改任务，返回修改后的文件字节。 */
export async function runEditInHiddenViewer(job: ClaimedJob, source: ArrayBuffer): Promise<ArrayBuffer> {
    const viewerUrl = resolveOfficeViewerUrl();
    const targetOrigin = viewerTargetOrigin(viewerUrl);
    const iframe = document.createElement("iframe");
    iframe.setAttribute("aria-hidden", "true");
    iframe.tabIndex = -1;
    iframe.title = "Office 修改任务";
    // 不可见但保留正常尺寸：编辑器需要布局才能打开文档
    iframe.style.cssText =
        "position:fixed;left:-20000px;top:0;width:1024px;height:768px;border:0;opacity:0;pointer-events:none;";
    const requestId = Math.floor(Math.random() * 1e9) + 1;

    return new Promise<ArrayBuffer>((resolve, reject) => {
        let settled = false;
        let readyTimer: number | undefined;
        let jobTimer: number | undefined;
        const cleanup = () => {
            window.removeEventListener("message", onMessage);
            window.clearTimeout(readyTimer);
            window.clearTimeout(jobTimer);
            iframe.remove();
        };
        const done = (fn: () => void) => {
            if (settled) return;
            settled = true;
            cleanup();
            fn();
        };
        const onMessage = (event: MessageEvent) => {
            if (event.source !== iframe.contentWindow) return;
            const data = event.data as {
                type?: string;
                requestId?: number;
                protocol?: number;
                ok?: boolean;
                bytes?: unknown;
                failure?: { code?: string; message?: string };
            } | null;
            if (!data || typeof data.type !== "string") return;
            if (data.type === "aipp-office:viewer-ready") {
                window.clearTimeout(readyTimer);
                if ((data.protocol ?? 1) < EDIT_PROTOCOL) {
                    done(() => reject(new OfficeJobError("plugin_unavailable", "Office 组件版本过旧，不支持修改，请在插件中心更新")));
                    return;
                }
                iframe.contentWindow?.postMessage(
                    { type: "aipp-office:edit-job", requestId, format: job.format, fileName: job.fileName, bytes: source, steps: job.steps },
                    targetOrigin,
                    [source],
                );
                jobTimer = window.setTimeout(
                    () => done(() => reject(new OfficeJobError("timeout", "Office 组件处理超时，未生成文件"))),
                    JOB_TIMEOUT_MS,
                );
                return;
            }
            if (data.type !== "aipp-office:edit-result" || data.requestId !== requestId) return;
            if (data.ok && data.bytes instanceof ArrayBuffer) {
                const bytes = data.bytes;
                done(() => resolve(bytes));
            } else {
                const failure = data.failure ?? {};
                done(() => reject(new OfficeJobError(failure.code || "edit_failed", failure.message || "Office 组件没有说明失败原因")));
            }
        };
        window.addEventListener("message", onMessage);
        readyTimer = window.setTimeout(
            () => done(() => reject(new OfficeJobError("plugin_unavailable", "Office 组件 30 秒内没有就绪"))),
            VIEWER_READY_TIMEOUT_MS,
        );
        iframe.src = viewerUrl;
        document.body.appendChild(iframe);
    });
}

async function runJob(job: ClaimedJob): Promise<void> {
    try {
        await ensureEditPlugin();
        const raw = await invoke<ArrayBuffer>("read_office_edit_job_source", { jobId: job.jobId });
        const payload = parseOfficeFileResponse(raw);
        const output = await runEditInHiddenViewer(job, payload.bytes);
        await invoke("complete_office_edit_job", encodeFramedRequest({ jobId: job.jobId }, output));
    } catch (err) {
        const code = err instanceof OfficeJobError ? err.code : "edit_failed";
        const message = err instanceof Error ? err.message : String(err);
        console.error("[officeEditRunner] job failed:", code, message);
        await invoke("fail_office_edit_job", { jobId: job.jobId, code, message }).catch((e) =>
            console.warn("[officeEditRunner] fail_office_edit_job failed:", e),
        );
    }
}

let started = false;
let queue: Promise<void> = Promise.resolve();

/** 每个窗口调用一次；返回停止监听的函数。 */
export function startOfficeEditRunner(): () => void {
    if (started) return () => {};
    started = true;
    let unlisten: (() => void) | null = null;
    let stopped = false;
    void listen<EditRequestEvent>(OFFICE_EDIT_EVENT, (event) => {
        const jobId = event.payload?.jobId;
        if (!jobId) return;
        void invoke<ClaimedJob | null>("claim_office_edit_job", { jobId })
            .then((job) => {
                if (!job) return; // 其他窗口已认领
                // 同一窗口里的任务依次执行，避免同时打开多个编辑器占用内存
                queue = queue.then(() => runJob(job));
            })
            .catch((err) => console.warn("[officeEditRunner] claim failed:", err));
    }).then((fn) => {
        if (stopped) fn();
        else unlisten = fn;
    });
    return () => {
        stopped = true;
        started = false;
        unlisten?.();
    };
}
