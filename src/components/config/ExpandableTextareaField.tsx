import React, { useState } from "react";
import { Maximize2 } from "lucide-react";
import { Textarea } from "@/components/ui/textarea";
import { Button } from "@/components/ui/button";
import {
    Dialog,
    DialogContent,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";

interface ExpandableTextareaFieldProps {
    label: string;
    className?: string;
    placeholder?: string;
    disabled?: boolean;
    fieldRenderData: any;
}

const ExpandableTextareaField: React.FC<ExpandableTextareaFieldProps> = ({
    label,
    className,
    placeholder,
    disabled,
    fieldRenderData,
}) => {
    const [open, setOpen] = useState(false);

    // 大文本框只复用 value/onChange/onBlur，避免和小文本框共用同一个 ref
    const { ref: _ref, ...bindable } = fieldRenderData ?? {};

    return (
        <>
            <div className="relative">
                <Textarea
                    className={`focus:ring-ring/20 focus:border-ring pr-9 ${className || ""}`}
                    disabled={disabled}
                    placeholder={placeholder}
                    {...fieldRenderData}
                />
                <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    disabled={disabled}
                    onClick={() => setOpen(true)}
                    className="absolute top-1.5 right-1.5 h-7 w-7 text-muted-foreground hover:text-foreground"
                    aria-label="扩展编辑"
                >
                    <Maximize2 className="h-4 w-4" />
                </Button>
            </div>

            <Dialog open={open} onOpenChange={setOpen}>
                <DialogContent className="w-[70vw] max-w-[70vw] h-[80vh] max-h-[80vh] flex flex-col overflow-hidden">
                    <DialogHeader className="flex-shrink-0">
                        <DialogTitle>{label}</DialogTitle>
                    </DialogHeader>
                    <Textarea
                        className="flex-1 min-h-0 resize-none focus:ring-ring/20 focus:border-ring"
                        disabled={disabled}
                        placeholder={placeholder}
                        autoFocus
                        {...bindable}
                    />
                </DialogContent>
            </Dialog>
        </>
    );
};

export default ExpandableTextareaField;
