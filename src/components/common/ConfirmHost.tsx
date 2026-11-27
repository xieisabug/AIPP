import { useEffect, useState } from "react";
import {
    AlertDialog,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";

export interface ConfirmOptions {
    title: string;
    /** 正文，保留换行。 */
    description: string;
    confirmText?: string;
    cancelText?: string;
    /** 危险操作（删除等）用红色确认键；默认是普通主按钮。 */
    destructive?: boolean;
}

interface Pending extends ConfirmOptions {
    resolve: (confirmed: boolean) => void;
}

const queue: Pending[] = [];
const listeners = new Set<() => void>();
const notify = () => listeners.forEach((listener) => listener());

/**
 * 应用内确认框，代替系统原生对话框（`window.confirm`、`@tauri-apps/plugin-dialog` 的 `ask`/`message`）。
 * 任何地方都可以 `await confirmDialog({...})`；同时多次调用时依次显示。
 * 需要窗口根挂载一个 `<ConfirmHost />`（App.tsx 已挂载）。
 */
export function confirmDialog(options: ConfirmOptions): Promise<boolean> {
    return new Promise((resolve) => {
        queue.push({ ...options, resolve });
        notify();
    });
}

export function ConfirmHost() {
    const [current, setCurrent] = useState<Pending | undefined>(queue[0]);
    useEffect(() => {
        const sync = () => setCurrent(queue[0]);
        listeners.add(sync);
        sync();
        return () => {
            listeners.delete(sync);
        };
    }, []);

    const settle = (confirmed: boolean) => {
        const head = queue.shift();
        head?.resolve(confirmed);
        notify();
    };

    return (
        <AlertDialog open={!!current} onOpenChange={(open) => { if (!open && current) settle(false); }}>
            {current && (
                <AlertDialogContent className="flex flex-col gap-0 overflow-hidden p-0 sm:max-w-md" data-aipp-slot="confirm-dialog">
                    <AlertDialogHeader className="border-b border-border px-4 py-3 text-left sm:px-5">
                        <AlertDialogTitle className="text-base font-semibold">{current.title}</AlertDialogTitle>
                    </AlertDialogHeader>
                    <div className="max-h-[60vh] overflow-y-auto px-4 py-4 sm:px-5">
                        <AlertDialogDescription className="whitespace-pre-wrap break-words text-sm leading-relaxed text-muted-foreground">
                            {current.description}
                        </AlertDialogDescription>
                    </div>
                    <AlertDialogFooter className="flex-row items-center justify-end gap-2 border-t border-border bg-background px-4 pt-2.5 pb-[calc(0.625rem+env(safe-area-inset-bottom))] sm:px-5">
                        <Button variant="outline" className="min-h-11 px-4 sm:min-h-9" onClick={() => settle(false)}
                            data-aipp-slot="confirm-dialog-cancel">
                            {current.cancelText ?? "取消"}
                        </Button>
                        <Button variant={current.destructive ? "destructive" : "default"} className="min-h-11 px-5 sm:min-h-9"
                            onClick={() => settle(true)} data-aipp-slot="confirm-dialog-confirm">
                            {current.confirmText ?? "确认"}
                        </Button>
                    </AlertDialogFooter>
                </AlertDialogContent>
            )}
        </AlertDialog>
    );
}
