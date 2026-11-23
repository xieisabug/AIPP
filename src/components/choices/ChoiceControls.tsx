import type { ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { Toggle } from "@/components/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { cn } from "@/utils/utils";

/** 触屏端保持 44px 点击区，桌面端用紧凑尺寸。 */
function useControlSize() {
    return useIsMobilePlatform() ? "size-11" : "size-8";
}

/**
 * TooltipTrigger 以 asChild 合并属性时会把自身的 data-state 盖到 Toggle 上，
 * 所以开关的选中态改用 aria-checked / aria-pressed 着色。
 */
const ACTIVE = "aria-checked:bg-accent aria-checked:text-accent-foreground aria-pressed:bg-accent aria-pressed:text-accent-foreground";

function Hint({ label, children }: { label: string; children: ReactNode }) {
    return (
        <Tooltip>
            <TooltipTrigger asChild>{children}</TooltipTrigger>
            <TooltipContent>{label}</TooltipContent>
        </Tooltip>
    );
}

/** 单个动作：仅图标，名称放在 aria-label 与悬浮提示里。 */
export function ChoiceIconButton({
    label,
    onClick,
    disabled,
    children,
}: {
    label: string;
    onClick: () => void;
    disabled?: boolean;
    children: ReactNode;
}) {
    const size = useControlSize();
    return (
        <Hint label={label}>
            <Button
                type="button"
                variant="ghost"
                size="icon"
                className={cn(size, "text-muted-foreground hover:text-foreground")}
                aria-label={label}
                disabled={disabled}
                onClick={onClick}
            >
                {children}
            </Button>
        </Hint>
    );
}

/** 独立的开/关状态，例如文本差异。 */
export function ChoiceToggle({
    label,
    pressed,
    onPressedChange,
    children,
}: {
    label: string;
    pressed: boolean;
    onPressedChange: (pressed: boolean) => void;
    children: ReactNode;
}) {
    const size = useControlSize();
    return (
        <Hint label={label}>
            <Toggle
                variant="outline"
                size="sm"
                className={cn(size, ACTIVE)}
                aria-label={label}
                pressed={pressed}
                onPressedChange={onPressedChange}
            >
                {children}
            </Toggle>
        </Hint>
    );
}

export type SegmentOption<T extends string> = { value: T; label: string; icon: ReactNode };

/** 互斥选项合成一组，例如视图布局、预览宽度。 */
export function ChoiceSegmented<T extends string>({
    label,
    value,
    options,
    onChange,
}: {
    label: string;
    value: T;
    options: SegmentOption<T>[];
    onChange: (value: T) => void;
}) {
    const size = useControlSize();
    return (
        <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            aria-label={label}
            value={value}
            onValueChange={(next) => next && onChange(next as T)}
        >
            {options.map((option) => (
                <Hint key={option.value} label={option.label}>
                    <ToggleGroupItem
                        value={option.value}
                        aria-label={option.label}
                        className={cn(size, "flex-none", ACTIVE)}
                    >
                        {option.icon}
                    </ToggleGroupItem>
                </Hint>
            ))}
        </ToggleGroup>
    );
}
