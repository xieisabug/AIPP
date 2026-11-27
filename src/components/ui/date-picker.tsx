import * as React from "react";
import { format, isValid, parse } from "date-fns";
import { CalendarIcon } from "lucide-react";
import type { Matcher } from "react-day-picker";
import { zhCN } from "react-day-picker/locale";

import { cn } from "@/utils/utils";
import { Button } from "@/components/ui/button";
import { Calendar } from "@/components/ui/calendar";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";

const VALUE_FORMAT = "yyyy-MM-dd";

/** 把 `yyyy-MM-dd` 解析成本地时区零点；格式不对时返回 undefined（避免 `new Date("yyyy-MM-dd")` 按 UTC 解析造成的日期偏移）。 */
export function parseDateValue(value: string | null | undefined): Date | undefined {
    if (!value || !/^\d{4}-\d{2}-\d{2}$/.test(value)) return undefined;
    const date = parse(value, VALUE_FORMAT, new Date());
    return isValid(date) ? date : undefined;
}

/** 把日期按本地时区格式化成 `yyyy-MM-dd`。 */
export function formatDateValue(date: Date): string {
    return format(date, VALUE_FORMAT);
}

export interface DatePickerProps {
    /** `yyyy-MM-dd`，空字符串表示未选择。 */
    value: string;
    onChange: (value: string) => void;
    placeholder?: string;
    className?: string;
    "aria-label"?: string;
    disabled?: boolean;
    /** 不可选的日期，例如 `{ before: new Date() }`。 */
    disabledDates?: Matcher | Matcher[];
    /** 显示「清除」按钮，允许把值清空。 */
    clearable?: boolean;
    /** 触发按钮上的显示格式（date-fns 格式串）。 */
    displayFormat?: string;
}

/** 应用内日期选择器：Popover + Calendar，代替会弹出系统日期面板的 `<input type="date">`。 */
function DatePicker({
    value,
    onChange,
    placeholder = "选择日期",
    className,
    "aria-label": ariaLabel,
    disabled,
    disabledDates,
    clearable = false,
    displayFormat = "yyyy年M月d日",
}: DatePickerProps) {
    const [open, setOpen] = React.useState(false);
    const selected = parseDateValue(value);

    return (
        <Popover open={open} onOpenChange={setOpen}>
            <PopoverTrigger asChild>
                <Button
                    type="button"
                    variant="outline"
                    disabled={disabled}
                    aria-label={ariaLabel}
                    data-empty={!selected || undefined}
                    className={cn(
                        "justify-start gap-2 px-3 font-normal data-[empty=true]:text-muted-foreground",
                        className
                    )}
                >
                    <CalendarIcon className="size-4 shrink-0 text-muted-foreground" />
                    <span className="truncate">{selected ? format(selected, displayFormat, { locale: zhCN }) : placeholder}</span>
                </Button>
            </PopoverTrigger>
            <PopoverContent className="w-auto p-0" align="start">
                <Calendar
                    mode="single"
                    locale={zhCN}
                    selected={selected}
                    defaultMonth={selected}
                    disabled={disabledDates}
                    onSelect={(date) => {
                        // 再次点击已选日期会被 DayPicker 视为取消选择；这里保持原值，清空走「清除」按钮。
                        if (!date) return;
                        onChange(formatDateValue(date));
                        setOpen(false);
                    }}
                />
                {clearable && selected && (
                    <div className="flex justify-end border-t border-border px-3 py-2">
                        <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            className="h-7 text-muted-foreground"
                            onClick={() => {
                                onChange("");
                                setOpen(false);
                            }}
                        >
                            清除
                        </Button>
                    </div>
                )}
            </PopoverContent>
        </Popover>
    );
}

export { DatePicker };
