import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { ExternalLink, Loader2 } from "lucide-react";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
    detectOfficeExtension,
    officeFormatLabel,
    OPEN_OFFICE_PREVIEW_EVENT,
    type OfficePreviewTarget,
} from "@/utils/officeFile";
import OfficePreviewPane, { OfficeFormatIcon } from "./OfficePreviewPane";

/**
 * 全局唯一的 Office 预览窗口，在 App 根部挂载一次；任意入口通过
 * `openOfficePreviewModal()` 派发事件打开。同一时刻只有一个查看器实例，关闭即卸载释放内存。
 */
export const OfficePreviewModal: React.FC = () => {
    const [target, setTarget] = useState<OfficePreviewTarget | null>(null);
    const [openingExternal, setOpeningExternal] = useState(false);

    useEffect(() => {
        const handler = (event: Event) => {
            const detail = (event as CustomEvent<OfficePreviewTarget>).detail;
            if (!detail || !detail.source) return;
            setTarget(detail);
        };
        window.addEventListener(OPEN_OFFICE_PREVIEW_EVENT, handler);
        return () => window.removeEventListener(OPEN_OFFICE_PREVIEW_EVENT, handler);
    }, []);

    const ext = target?.extension ?? detectOfficeExtension(target?.title) ?? detectOfficeExtension(target?.source.localPath);

    const handleOpenExternally = async () => {
        if (!target) return;
        setOpeningExternal(true);
        try {
            await invoke("open_office_file_externally", { source: target.source });
        } catch (err) {
            const msg = err instanceof Error ? err.message : String(err);
            toast.error(`调用外部应用失败：${msg}`);
        } finally {
            setOpeningExternal(false);
        }
    };

    return (
        <Dialog open={Boolean(target)} onOpenChange={(open) => !open && setTarget(null)}>
            <DialogContent className="flex max-h-[92vh] w-[95vw] max-w-5xl sm:max-w-5xl flex-col gap-0 overflow-hidden p-0">
                <DialogHeader className="flex flex-row flex-wrap items-center justify-between gap-2 border-b border-border/60 px-4 py-3 pr-12 text-left">
                    <div className="flex min-w-0 flex-1 items-center gap-2">
                        <OfficeFormatIcon ext={ext} className="h-4 w-4 shrink-0 text-muted-foreground" />
                        <DialogTitle className="truncate text-sm font-medium">
                            {target?.title || "Office 文档预览"}
                        </DialogTitle>
                        <Badge variant="secondary" className="shrink-0 text-[11px] font-normal">
                            只读预览
                        </Badge>
                    </div>
                    <DialogDescription className="sr-only">
                        {officeFormatLabel(ext)} 统一只读预览，不修改原文件
                    </DialogDescription>
                    <div className="flex shrink-0 items-center gap-1.5">
                        <Button
                            type="button"
                            variant="outline"
                            size="sm"
                            onClick={() => void handleOpenExternally()}
                            disabled={openingExternal}
                            className="h-8 gap-1.5 text-xs"
                        >
                            {openingExternal ? (
                                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                            ) : (
                                <ExternalLink className="h-3.5 w-3.5" />
                            )}
                            用外部应用打开
                        </Button>
                    </div>
                </DialogHeader>
                {target && (
                    <OfficePreviewPane
                        title={target.title}
                        source={target.source}
                        heightClassName="h-[78vh]"
                    />
                )}
            </DialogContent>
        </Dialog>
    );
};

export default OfficePreviewModal;
