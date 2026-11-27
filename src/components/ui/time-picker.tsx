import * as React from "react";
import { Clock } from "lucide-react";

import { cn } from "@/utils/utils";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";

const pad = (value: number) => String(value).padStart(2, "0");

/** 解析 `HH:mm`（小时允许一位）；不合法返回 null。 */
export function parseTimeValue(value: string | null | undefined): { hour: number; minute: number } | null {
    const match = /^(\d{1,2}):(\d{2})$/.exec((value ?? "").trim());
    if (!match) return null;
    const hour = Number(match[1]);
    const minute = Number(match[2]);
    return hour < 24 && minute < 60 ? { hour, minute } : null;
}

export function formatTimeValue(hour: number, minute: number): string {
    return `${pad(hour)}:${pad(minute)}`;
}

const HOURS = Array.from({ length: 24 }, (_, hour) => hour);

function TimeColumn({
    label,
    values,
    selected,
    onSelect,
}: {
    label: string;
    values: number[];
    selected: number | null;
    onSelect: (value: number) => void;
}) {
    const listRef = React.useRef<HTMLDivElement>(null);

    // 打开时把选中项滚到列中间；只改这一列的 scrollTop，不牵动外层弹窗滚动。
    React.useEffect(() => {
        const list = listRef.current;
        const option = list?.querySelector<HTMLElement>('[aria-selected="true"]');
        if (list && option) list.scrollTop = option.offsetTop - list.clientHeight / 2 + option.clientHeight / 2;
    }, []);

    const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
        const options = Array.from(listRef.current?.querySelectorAll<HTMLElement>('[role="option"]') ?? []);
        const index = options.indexOf(document.activeElement as HTMLElement);
        const next = {
            ArrowDown: index + 1,
            ArrowUp: index - 1,
            Home: 0,
            End: options.length - 1,
        }[event.key as "ArrowDown" | "ArrowUp" | "Home" | "End"];
        if (next === undefined) return;
        event.preventDefault();
        options[Math.max(0, Math.min(options.length - 1, next))]?.focus();
    };

    const focusable = selected !== null && values.includes(selected) ? selected : values[0];
    return (
        <div
            ref={listRef}
            role="listbox"
            aria-label={label}
            className="relative flex max-h-56 w-14 flex-col gap-0.5 overflow-y-auto p-1"
            onKeyDown={onKeyDown}
        >
            {values.map((value) => (
                <button
                    key={value}
                    type="button"
                    role="option"
                    aria-selected={value === selected}
                    tabIndex={value === focusable ? 0 : -1}
                    onClick={() => onSelect(value)}
                    className="shrink-0 rounded-md px-2 py-1 text-center text-sm tabular-nums outline-none hover:bg-accent hover:text-accent-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50 aria-selected:bg-primary aria-selected:text-primary-foreground"
                >
                    {pad(value)}
                </button>
            ))}
        </div>
    );
}

export interface TimePickerProps {
    /** 24 小时制 `HH:mm`，空字符串表示未设置。 */
    value: string;
    onChange: (value: string) => void;
    placeholder?: string;
    className?: string;
    "aria-label"?: string;
    disabled?: boolean;
    /** 分钟列的步长；当前值不在步长上时也会列出，避免显示不出来。 */
    minuteStep?: number;
}

/** 应用内时间选择器：Popover + 时、分两列，代替会弹出系统时间面板的 `<input type="time">`。 */
function TimePicker({
    value,
    onChange,
    placeholder = "--:--",
    className,
    "aria-label": ariaLabel,
    disabled,
    minuteStep = 1,
}: TimePickerProps) {
    const [open, setOpen] = React.useState(false);
    const parsed = parseTimeValue(value);

    const minutes = React.useMemo(() => {
        const step = Math.max(1, Math.floor(minuteStep));
        const list = Array.from({ length: Math.ceil(60 / step) }, (_, index) => index * step);
        if (parsed && !list.includes(parsed.minute)) list.push(parsed.minute);
        return list.sort((a, b) => a - b);
    }, [minuteStep, parsed?.minute]);

    return (
        <Popover open={open} onOpenChange={setOpen}>
            <PopoverTrigger asChild>
                <Button
                    type="button"
                    variant="outline"
                    disabled={disabled}
                    aria-label={ariaLabel}
                    data-empty={!parsed || undefined}
                    className={cn(
                        "justify-start gap-2 px-3 font-normal tabular-nums data-[empty=true]:text-muted-foreground",
                        className
                    )}
                >
                    <Clock className="size-4 shrink-0 text-muted-foreground" />
                    <span className="truncate">{parsed ? formatTimeValue(parsed.hour, parsed.minute) : placeholder}</span>
                </Button>
            </PopoverTrigger>
            <PopoverContent className="flex w-auto divide-x divide-border p-0" align="start">
                <TimeColumn
                    label="小时"
                    values={HOURS}
                    selected={parsed?.hour ?? null}
                    onSelect={(hour) => onChange(formatTimeValue(hour, parsed?.minute ?? 0))}
                />
                <TimeColumn
                    label="分钟"
                    values={minutes}
                    selected={parsed?.minute ?? null}
                    onSelect={(minute) => {
                        onChange(formatTimeValue(parsed?.hour ?? 0, minute));
                        setOpen(false);
                    }}
                />
            </PopoverContent>
        </Popover>
    );
}

export { TimePicker };
