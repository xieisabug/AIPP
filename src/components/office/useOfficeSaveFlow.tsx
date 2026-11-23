import { useCallback, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { Loader2 } from "lucide-react";
import {
    AlertDialog,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import {
    isAndroid,
    OFFICE_FORMAT_SAVE_NOTES,
    OfficeSaveError,
    pickOfficeSaveTarget,
    saveOfficeOnAndroid,
    shareOfficeOnAndroid,
    writeOfficeCopy,
    type OfficeSaveRequest,
    type OfficeSaveResult,
} from "@/services/officeSave";

export type OfficeSaveOutcome =
    | { status: "saved"; fileName: string; location: string }
    | { status: "cancelled" };

type Step =
    | { kind: "notice" }
    | { kind: "saving" }
    | { kind: "confirm-overwrite"; target: string }
    | { kind: "error"; title: string; message: string };

/** `报告.docx` → `报告（副本）.docx`（与 Rust `copy_file_name` 一致）。 */
export function defaultCopyName(fileName: string, purpose: OfficeSaveRequest["purpose"]): string {
    if (purpose === "output") return fileName;
    const dot = fileName.lastIndexOf(".");
    return dot > 0 ? `${fileName.slice(0, dot)}（副本）${fileName.slice(dot)}` : `${fileName}（副本）`;
}

function baseName(path: string): string {
    return path.split(/[\\/]/).pop() || path;
}

/**
 * 另存副本的完整流程：格式限制说明 → 选择位置 → 写入（已存在时二次确认才覆盖）→ 结果。
 * `startSave` 在流程结束时返回结果；取消、失败都不会被当成“已保存”。
 */
export function useOfficeSaveFlow() {
    const [request, setRequest] = useState<OfficeSaveRequest | null>(null);
    const [step, setStep] = useState<Step>({ kind: "notice" });
    const [mobile, setMobile] = useState(false);
    const resolverRef = useRef<((outcome: OfficeSaveOutcome) => void) | null>(null);

    const finish = useCallback((outcome: OfficeSaveOutcome) => {
        resolverRef.current?.(outcome);
        resolverRef.current = null;
        setRequest(null);
        setStep({ kind: "notice" });
    }, []);

    const startSave = useCallback((next: OfficeSaveRequest): Promise<OfficeSaveOutcome> => {
        resolverRef.current?.({ status: "cancelled" });
        void isAndroid().then(setMobile).catch(() => setMobile(false));
        setRequest(next);
        setStep({ kind: "notice" });
        return new Promise<OfficeSaveOutcome>((resolve) => {
            resolverRef.current = resolve;
        });
    }, []);

    const showSaved = useCallback((result: OfficeSaveResult) => {
        toast.success(`已另存为「${result.fileName}」`, {
            description: "原文件未改动。",
            action: {
                label: "打开",
                onClick: () => void openPath(result.path).catch((err) => toast.error(`打开失败：${String(err)}`)),
            },
        });
        finish({ status: "saved", fileName: result.fileName, location: result.path });
    }, [finish]);

    const fail = useCallback((err: unknown) => {
        const e = err instanceof OfficeSaveError ? err : new OfficeSaveError({ code: "write_failed", title: "保存失败", message: String(err) });
        setStep({ kind: "error", title: e.title, message: e.message });
    }, []);

    const write = useCallback(async (target: string, overwrite: boolean) => {
        if (!request) return;
        setStep({ kind: "saving" });
        try {
            showSaved(await writeOfficeCopy(request, target, overwrite));
        } catch (err) {
            if (!overwrite && err instanceof OfficeSaveError && err.code === "target_exists") {
                setStep({ kind: "confirm-overwrite", target });
                return;
            }
            fail(err);
        }
    }, [request, showSaved, fail]);

    const chooseAndSave = useCallback(async () => {
        if (!request) return;
        if (mobile) {
            setStep({ kind: "saving" });
            const name = defaultCopyName(request.fileName, request.purpose);
            try {
                const result = await saveOfficeOnAndroid(request, name);
                if (result.status === "cancelled") {
                    setStep({ kind: "notice" });
                    return;
                }
                if (result.status !== "saved" || !result.uri) {
                    fail(new OfficeSaveError({ code: "write_failed", title: "保存未完成", message: `系统返回 ${result.status}，文件没有保存` }));
                    return;
                }
                toast.success(`已保存到本地「${name}」`, {
                    description: "原文件未改动。",
                    action: {
                        label: "分享",
                        onClick: () =>
                            void shareOfficeOnAndroid(request, name)
                                .then(() => toast.info("已调起系统分享，是否发送成功请在目标应用中确认"))
                                .catch((err) => toast.error(String(err))),
                    },
                });
                finish({ status: "saved", fileName: name, location: result.uri });
            } catch (err) {
                fail(err);
            }
            return;
        }
        let target: string | null;
        try {
            target = await pickOfficeSaveTarget(request);
        } catch (err) {
            fail(err);
            return;
        }
        if (!target) return; // 用户在对话框里取消：停在说明页，可以再选
        await write(target, false);
    }, [request, mobile, write, fail, finish]);

    const busy = step.kind === "saving";
    const ext = request?.format ?? "docx";
    const dialog = (
        <AlertDialog open={request !== null} onOpenChange={(open) => !open && !busy && finish({ status: "cancelled" })}>
            <AlertDialogContent className="flex flex-col gap-0 overflow-hidden p-0 sm:max-w-md" data-testid="office-save-dialog">
                {step.kind === "confirm-overwrite" ? (
                    <>
                        <AlertDialogHeader className="border-b border-border px-4 py-3 text-left sm:px-5">
                            <AlertDialogTitle className="truncate text-base font-semibold">覆盖「{baseName(step.target)}」？</AlertDialogTitle>
                        </AlertDialogHeader>
                        <div className="px-4 py-4 sm:px-5">
                            <AlertDialogDescription className="text-sm leading-relaxed text-muted-foreground">
                                这个位置已经有同名文件，覆盖会替换它的内容。新内容先写入临时文件，完整写好后才替换；写入失败时原有文件保持不变。
                            </AlertDialogDescription>
                        </div>
                        <AlertDialogFooter className="flex-row items-center justify-end gap-2 border-t border-border bg-background px-4 pt-2.5 pb-[calc(0.625rem+env(safe-area-inset-bottom))] sm:px-5">
                            <Button variant="outline" className="min-h-11 px-4 sm:min-h-9" onClick={() => setStep({ kind: "notice" })}>
                                换个位置
                            </Button>
                            <Button variant="destructive" className="min-h-11 px-5 sm:min-h-9" onClick={() => void write(step.target, true)} data-testid="office-save-overwrite">
                                覆盖
                            </Button>
                        </AlertDialogFooter>
                    </>
                ) : step.kind === "error" ? (
                    <>
                        <AlertDialogHeader className="border-b border-border px-4 py-3 text-left sm:px-5">
                            <AlertDialogTitle className="text-base font-semibold">{step.title}</AlertDialogTitle>
                        </AlertDialogHeader>
                        <div className="px-4 py-4 sm:px-5">
                            <AlertDialogDescription className="break-words text-sm leading-relaxed text-muted-foreground">
                                {step.message}。原文件未受影响，没有生成不完整的文件。
                            </AlertDialogDescription>
                        </div>
                        <AlertDialogFooter className="flex-row items-center justify-end gap-2 border-t border-border bg-background px-4 pt-2.5 pb-[calc(0.625rem+env(safe-area-inset-bottom))] sm:px-5">
                            <Button variant="outline" className="min-h-11 px-4 sm:min-h-9" onClick={() => finish({ status: "cancelled" })}>
                                关闭
                            </Button>
                            <Button className="min-h-11 px-5 sm:min-h-9" onClick={() => void chooseAndSave()}>重新选择位置</Button>
                        </AlertDialogFooter>
                    </>
                ) : (
                    <>
                        <AlertDialogHeader className="border-b border-border px-4 py-3 text-left sm:px-5">
                            <AlertDialogTitle className="text-base font-semibold">另存为新文件</AlertDialogTitle>
                        </AlertDialogHeader>
                        <div className="px-4 py-4 sm:px-5">
                            <AlertDialogDescription asChild>
                                <div className="space-y-2 text-sm leading-relaxed text-muted-foreground">
                                    <p>
                                        {request?.purpose === "output" ? "这份修改后的副本" : "修改后的内容"}会保存为新文件，原文件「{request?.fileName}」保持不变。
                                    </p>
                                    <p>{OFFICE_FORMAT_SAVE_NOTES[ext]}</p>
                                    <p>另存的文件可在 Microsoft Office 与 WPS 中打开；复杂排版与特性不保证完全无损。</p>
                                </div>
                            </AlertDialogDescription>
                        </div>
                        <AlertDialogFooter className="flex-row items-center justify-end gap-2 border-t border-border bg-background px-4 pt-2.5 pb-[calc(0.625rem+env(safe-area-inset-bottom))] sm:px-5">
                            <Button variant="outline" className="min-h-11 px-4 sm:min-h-9" disabled={busy} onClick={() => finish({ status: "cancelled" })}>
                                取消
                            </Button>
                            <Button className="min-h-11 px-5 sm:min-h-9" disabled={busy} onClick={() => void chooseAndSave()} data-testid="office-save-choose">
                                {busy && <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />}
                                {mobile ? "保存到手机" : "选择位置并保存"}
                            </Button>
                        </AlertDialogFooter>
                    </>
                )}
            </AlertDialogContent>
        </AlertDialog>
    );

    return { startSave, dialog };
}

/** 外部打开已授权文件（AI 副本等）。 */
export async function openOfficeSourceExternally(source: OfficeSaveRequest["source"]): Promise<void> {
    await invoke("open_office_file_externally", { source });
}

export default useOfficeSaveFlow;
