import { useCallback, useEffect, useRef, useState } from "react";
import { registerConfigBeforeLeave } from "@/lib/configLeaveGuard";
import {
    AlertDialog,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";

interface UseUnsavedLeaveGuardOptions {
    isDirty: boolean;
    onSave: () => Promise<void> | void;
    onDiscard: () => void;
    /** Disable registration when embedded in a secondary dialog or inactive tab. */
    enabled?: boolean;
}

/**
 * Registers `configLeaveGuard` while a settings form has unsaved edits and returns
 * a ready-to-render confirmation dialog supporting Save / Discard / Cancel.
 */
export function useUnsavedLeaveGuard({
    isDirty,
    onSave,
    onDiscard,
    enabled = true,
}: UseUnsavedLeaveGuardOptions) {
    const isDirtyRef = useRef(isDirty);
    isDirtyRef.current = isDirty;

    const [prompt, setPromptState] = useState<null | { proceed: () => void }>(null);
    const promptRef = useRef<typeof prompt>(null);
    const [savingOnLeave, setSavingOnLeave] = useState(false);

    const setPrompt = useCallback((next: typeof prompt) => {
        promptRef.current = next;
        setPromptState(next);
    }, []);

    useEffect(() => () => {
        promptRef.current = null;
    }, []);

    useEffect(() => {
        if (!enabled) return;
        return registerConfigBeforeLeave((proceed) => {
            if (!isDirtyRef.current) {
                proceed();
                return;
            }
            setPrompt({ proceed });
        });
    }, [enabled, setPrompt]);

    const handleSaveAndLeave = useCallback(async () => {
        const current = promptRef.current;
        if (!current) return;
        setSavingOnLeave(true);
        try {
            await onSave();
            if (promptRef.current !== current) return;
            setPrompt(null);
            current.proceed();
        } catch {
            // Stay on page when save fails; caller shows toast.
        } finally {
            setSavingOnLeave(false);
        }
    }, [onSave, setPrompt]);

    const handleDiscardAndLeave = useCallback(() => {
        const current = promptRef.current;
        setPrompt(null);
        onDiscard();
        current?.proceed();
    }, [onDiscard, setPrompt]);

    const dialog = (
        <AlertDialog
            open={prompt !== null}
            onOpenChange={(open) => {
                if (!open && !savingOnLeave) setPrompt(null);
            }}
        >
            <AlertDialogContent>
                <AlertDialogHeader>
                    <AlertDialogTitle>有未保存的更改</AlertDialogTitle>
                    <AlertDialogDescription>
                        离开前是否保存当前页面的修改？放弃更改将恢复到上次保存的状态。
                    </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                    <Button
                        type="button"
                        variant="ghost"
                        disabled={savingOnLeave}
                        onClick={() => setPrompt(null)}
                    >
                        留在此页
                    </Button>
                    <Button
                        type="button"
                        variant="outline"
                        disabled={savingOnLeave}
                        onClick={handleDiscardAndLeave}
                    >
                        放弃更改
                    </Button>
                    <Button
                        type="button"
                        disabled={savingOnLeave}
                        onClick={() => void handleSaveAndLeave()}
                    >
                        {savingOnLeave ? "保存中…" : "保存并离开"}
                    </Button>
                </AlertDialogFooter>
            </AlertDialogContent>
        </AlertDialog>
    );

    return { dialog, hasLeavePrompt: prompt !== null };
}
