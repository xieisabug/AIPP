import React from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { X } from "lucide-react";
import { cn } from "@/utils/utils";
import ModelRequestModeToggle from "./ModelRequestModeToggle";
import { ModelTagItem } from "./llmModelTypes";

interface ModelTagBadgeProps {
    model: ModelTagItem;
    onRemove?: () => void;
    onToggleRequestMode?: (mode: string) => void;
    showRequestModeToggle?: boolean;
}

const ModelTagBadge: React.FC<ModelTagBadgeProps> = ({
    model,
    onRemove,
    onToggleRequestMode,
    showRequestModeToggle = false,
}) => {
    const withMode = showRequestModeToggle && !!onToggleRequestMode;
    return (
        <Badge
            variant="secondary"
            className={cn(
                "bg-background text-foreground border-border hover:border-muted-foreground/40 transition-colors py-1 text-sm font-normal",
                withMode ? "w-full justify-between gap-1 pl-3 pr-1" : "pl-3 pr-1",
                !onRemove && "pr-2",
            )}
        >
            <span className={cn(withMode && "min-w-0 flex-1 truncate")} title={model.name}>
                {model.name}
            </span>
            {withMode && (
                <ModelRequestModeToggle
                    requestMode={model.request_mode}
                    availableModes={model.available_modes}
                    onToggle={onToggleRequestMode!}
                />
            )}
            {onRemove && (
                <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    aria-label={`移除 ${model.name}`}
                    className="h-5 w-5 shrink-0 p-0 text-muted-foreground hover:bg-muted hover:text-foreground rounded-full"
                    onClick={onRemove}
                >
                    <X className="h-3 w-3" />
                </Button>
            )}
        </Badge>
    );
};

export default ModelTagBadge;
