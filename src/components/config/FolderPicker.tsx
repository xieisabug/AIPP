import React, { useCallback } from "react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { FolderOpen } from "lucide-react";
import { open } from '@tauri-apps/plugin-dialog';

interface FolderPickerProps {
    value: string;
    onChange: (value: string) => void;
    placeholder?: string;
    disabled?: boolean;
    className?: string;
}

export const FolderPicker: React.FC<FolderPickerProps> = ({
    value,
    onChange,
    placeholder = "选择工作目录",
    disabled = false,
    className = "",
}) => {
    const handleSelectFolder = useCallback(async () => {
        try {
            const selected = await open({
                directory: true,
                multiple: false,
            });

            if (selected && typeof selected === 'string') {
                onChange(selected);
            }
        } catch (error) {
            console.error('Failed to select folder:', error);
        }
    }, [onChange]);

    return (
        <div className={`flex items-center gap-2 ${className}`}>
            <Input
                type="text"
                value={value}
                onChange={(e) => onChange(e.target.value)}
                placeholder={placeholder}
                disabled={disabled}
                className="flex-1"
            />
            <Button
                type="button"
                variant="outline"
                size="icon"
                onClick={handleSelectFolder}
                disabled={disabled}
                className="shrink-0"
            >
                <FolderOpen className="h-4 w-4" />
            </Button>
        </div>
    );
};
