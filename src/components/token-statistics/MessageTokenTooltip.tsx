import { useState } from "react";
import { Popover, PopoverTrigger, PopoverContent } from "@/components/ui/popover";
import IconButton from "../IconButton";
import { Info } from "lucide-react";

interface MessageTokenTooltipProps {
    tokenCount: number;
    inputTokenCount: number;
    outputTokenCount: number;
    messageType: string;
    onOpenChange?: (open: boolean) => void;
}

export function MessageTokenTooltip({
    tokenCount,
    inputTokenCount,
    outputTokenCount,
    messageType,
    onOpenChange,
}: MessageTokenTooltipProps) {
    // 只在 response 类型消息上显示
    if (messageType !== "response") {
        return null;
    }

    const [isOpen, setIsOpen] = useState(false);

    const formatNumber = (num: number) => {
        return new Intl.NumberFormat("en-US").format(num);
    };

    return (
        <Popover
            open={isOpen}
            onOpenChange={(open) => {
                setIsOpen(open);
                onOpenChange?.(open);
            }}
        >
            <PopoverTrigger asChild>
                <IconButton icon={<Info className="h-4 w-4" />} onClick={() => {}} />
            </PopoverTrigger>
            <PopoverContent side="top" align="start" className="w-auto">
                <div className="space-y-2">
                    <div className="flex justify-between items-center gap-4 border-b pb-2">
                        <span className="text-sm font-medium">总 Token</span>
                        <span className="text-sm font-bold">
                            {formatNumber(tokenCount)}
                        </span>
                    </div>

                    <div className="space-y-1 text-sm">
                        <div className="flex justify-between gap-4">
                            <span className="text-muted-foreground">输入:</span>
                            <span className="font-medium">
                                {formatNumber(inputTokenCount)}
                            </span>
                        </div>
                        <div className="flex justify-between gap-4">
                            <span className="text-muted-foreground">输出:</span>
                            <span className="font-medium">
                                {formatNumber(outputTokenCount)}
                            </span>
                        </div>
                    </div>
                </div>
            </PopoverContent>
        </Popover>
    );
}
