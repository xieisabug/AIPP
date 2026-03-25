import React from "react";
import { Button } from "@/components/ui/button";
import {
    Tooltip,
    TooltipContent,
    TooltipProvider,
    TooltipTrigger,
} from "@/components/ui/tooltip";
import { getRequestModeLabel, getRequestModeTooltip } from "./llmModelTypes";

interface ModelRequestModeToggleProps {
    requestMode: string;
    onToggle: () => void;
    disabled?: boolean;
}

const ModelRequestModeToggle: React.FC<ModelRequestModeToggleProps> = ({
    requestMode,
    onToggle,
    disabled = false,
}) => (
    <TooltipProvider>
        <Tooltip>
            <TooltipTrigger asChild>
                <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    disabled={disabled}
                    className="h-4 w-4 p-0 hover:bg-muted-foreground/20 hover:text-foreground rounded-full ml-1 text-[10px] font-semibold lowercase"
                    onClick={(event) => {
                        event.stopPropagation();
                        onToggle();
                    }}
                >
                    {getRequestModeLabel(requestMode)}
                </Button>
            </TooltipTrigger>
            <TooltipContent>
                <p>{getRequestModeTooltip(requestMode)}</p>
            </TooltipContent>
        </Tooltip>
    </TooltipProvider>
);

export default ModelRequestModeToggle;
