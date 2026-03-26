import React from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { X } from "lucide-react";
import ModelRequestModeToggle from "./ModelRequestModeToggle";
import { ModelTagItem } from "./llmModelTypes";

interface ModelTagBadgeProps {
    model: ModelTagItem;
    onRemove?: () => void;
    onToggleRequestMode?: () => void;
    showRequestModeToggle?: boolean;
}

const ModelTagBadge: React.FC<ModelTagBadgeProps> = ({
    model,
    onRemove,
    onToggleRequestMode,
    showRequestModeToggle = false,
}) => (
    <Badge
        variant="secondary"
        className="bg-muted text-foreground border-border hover:bg-muted/80 transition-colors pl-3 pr-1 py-1 text-sm"
    >
        <span className="mr-2">{model.name}</span>
        {showRequestModeToggle && onToggleRequestMode && (
            <ModelRequestModeToggle
                requestMode={model.request_mode}
                onToggle={onToggleRequestMode}
            />
        )}
        {onRemove && (
            <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-4 w-4 p-0 hover:bg-muted-foreground/20 hover:text-foreground rounded-full ml-1"
                onClick={onRemove}
            >
                <X className="h-3 w-3" />
            </Button>
        )}
    </Badge>
);

export default ModelTagBadge;
