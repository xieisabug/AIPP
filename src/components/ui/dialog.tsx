import * as React from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { XIcon } from "lucide-react";

import { cn } from "@/utils/utils";

function Dialog({ ...props }: React.ComponentProps<typeof DialogPrimitive.Root>) {
    return <DialogPrimitive.Root data-slot="dialog" {...props} />;
}

function DialogTrigger({ ...props }: React.ComponentProps<typeof DialogPrimitive.Trigger>) {
    return <DialogPrimitive.Trigger data-slot="dialog-trigger" {...props} />;
}

function DialogPortal({ ...props }: React.ComponentProps<typeof DialogPrimitive.Portal>) {
    return <DialogPrimitive.Portal data-slot="dialog-portal" {...props} />;
}

function DialogClose({ ...props }: React.ComponentProps<typeof DialogPrimitive.Close>) {
    return <DialogPrimitive.Close data-slot="dialog-close" {...props} />;
}

function DialogOverlay({
    className,
    disableAnimation = false,
    ...props
}: React.ComponentProps<typeof DialogPrimitive.Overlay> & {
    disableAnimation?: boolean;
}) {
    return (
        <DialogPrimitive.Overlay
            data-slot="dialog-overlay"
            className={cn(
                "fixed inset-0 z-50 bg-black/50",
                !disableAnimation &&
                    "data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0",
                className
            )}
            {...props}
        />
    );
}

function DialogContent({
    className,
    children,
    showCloseButton = true,
    disableAnimation = false,
    ...props
}: React.ComponentProps<typeof DialogPrimitive.Content> & {
    showCloseButton?: boolean;
    disableAnimation?: boolean;
}) {
    return (
        <DialogPortal data-slot="dialog-portal">
            <DialogOverlay disableAnimation={disableAnimation} />
            <DialogPrimitive.Content
                data-slot="dialog-content"
                className={cn(
                    "bg-background fixed top-[50%] left-[50%] z-50 grid w-full max-w-[calc(100%-2rem)] translate-x-[-50%] translate-y-[-50%] gap-4 rounded-lg border p-6 shadow-lg sm:max-w-lg",
                    !disableAnimation &&
                        "data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 duration-200",
                    className
                )}
                {...props}
            >
                {children}
                {showCloseButton && (
                    <DialogPrimitive.Close
                        data-slot="dialog-close"
                        className="ring-offset-background focus:ring-ring data-[state=open]:bg-accent data-[state=open]:text-muted-foreground absolute top-4 right-4 rounded-xs opacity-70 transition-opacity hover:opacity-100 focus:ring-2 focus:ring-offset-2 focus:outline-hidden disabled:pointer-events-none [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4"
                    >
                        <XIcon />
                        <span className="sr-only">Close</span>
                    </DialogPrimitive.Close>
                )}
            </DialogPrimitive.Content>
        </DialogPortal>
    );
}

function DialogHeader({ className, ...props }: React.ComponentProps<"div">) {
    return (
        <div
            data-slot="dialog-header"
            className={cn("flex flex-col gap-2 text-center sm:text-left", className)}
            {...props}
        />
    );
}

function DialogFooter({ className, ...props }: React.ComponentProps<"div">) {
    return (
        <div
            data-slot="dialog-footer"
            className={cn("flex flex-col-reverse gap-2 sm:flex-row sm:justify-end", className)}
            {...props}
        />
    );
}

function DialogTitle({ className, ...props }: React.ComponentProps<typeof DialogPrimitive.Title>) {
    return (
        <DialogPrimitive.Title
            data-slot="dialog-title"
            className={cn("text-lg leading-none font-semibold", className)}
            {...props}
        />
    );
}

function DialogDescription({ className, ...props }: React.ComponentProps<typeof DialogPrimitive.Description>) {
    return (
        <DialogPrimitive.Description
            data-slot="dialog-description"
            className={cn("text-muted-foreground text-sm", className)}
            {...props}
        />
    );
}

/**
 * 紧凑型对话框容器：与新增模型提供商 / 新增助手向导一致的视觉框架（无默认内边距，由顶栏、可滚动主体、底栏组成）。
 */
function CompactDialogContent({
    className,
    children,
    disableAnimation = false,
    ...props
}: Omit<React.ComponentProps<typeof DialogPrimitive.Content>, "children"> & {
    children?: React.ReactNode;
    disableAnimation?: boolean;
}) {
    return (
        <DialogContent
            showCloseButton={false}
            disableAnimation={disableAnimation}
            data-aipp-dialog="compact"
            className={cn(
                "flex flex-col gap-0 overflow-hidden p-0 sm:max-w-lg !max-h-[min(88dvh,var(--aipp-vv-height,88dvh))] top-[max(1rem,var(--aipp-vv-offset-top,0px))] translate-y-0 data-[state=open]:slide-in-from-top-2 sm:top-[50%] sm:translate-y-[-50%]",
                className,
            )}
            {...props}
        >
            {children}
        </DialogContent>
    );
}

function CompactDialogHeader({
    title,
    subtitle,
    description,
    icon,
    actions,
    showCloseButton = true,
    closeDisabled = false,
    closeLabel = "关闭",
    className,
    children,
}: {
    title?: React.ReactNode;
    subtitle?: React.ReactNode;
    description?: string;
    icon?: React.ReactNode;
    actions?: React.ReactNode;
    showCloseButton?: boolean;
    closeDisabled?: boolean;
    closeLabel?: string;
    className?: string;
    children?: React.ReactNode;
}) {
    return (
        <div
            data-slot="compact-dialog-header"
            className={cn(
                "relative flex shrink-0 items-center gap-2.5 border-b border-border px-4 py-2.5 sm:px-5",
                className,
            )}
        >
            {icon && <span className="flex shrink-0 items-center text-muted-foreground">{icon}</span>}
            {title !== undefined && (
                <DialogTitle className="min-w-0 truncate text-base font-semibold">
                    {title}
                </DialogTitle>
            )}
            {subtitle && (
                <span className="truncate text-xs text-muted-foreground">{subtitle}</span>
            )}
            {children}
            {description !== undefined && (
                <DialogDescription className="sr-only">{description}</DialogDescription>
            )}
            {(actions || showCloseButton) && (
                <div className="ml-auto flex shrink-0 items-center gap-1.5">
                    {actions}
                    {showCloseButton && (
                        <DialogClose asChild>
                            <button
                                type="button"
                                aria-label={closeLabel}
                                disabled={closeDisabled}
                                className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-50"
                            >
                                <XIcon className="h-4 w-4" />
                            </button>
                        </DialogClose>
                    )}
                </div>
            )}
        </div>
    );
}

function CompactDialogBody({ className, ...props }: React.ComponentProps<"div">) {
    return (
        <div
            data-slot="compact-dialog-body"
            className={cn(
                "min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 py-4 sm:px-5",
                className,
            )}
            {...props}
        />
    );
}

function CompactDialogFooter({ className, ...props }: React.ComponentProps<"div">) {
    return (
        /* 底部内边距 = 顶部 + 安全区；不用 .safe-bottom（未分层 CSS 会把 pb 覆盖成 0） */
        <div
            data-slot="compact-dialog-footer"
            className={cn(
                "flex shrink-0 flex-wrap items-center justify-end gap-2 border-t border-border bg-background px-4 pt-2.5 pb-[calc(0.625rem+env(safe-area-inset-bottom))] sm:px-5",
                className,
            )}
            {...props}
        />
    );
}

export {
    Dialog,
    DialogClose,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogOverlay,
    DialogPortal,
    DialogTitle,
    DialogTrigger,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    CompactDialogFooter,
};
