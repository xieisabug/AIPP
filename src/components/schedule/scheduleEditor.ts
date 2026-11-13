/**
 * 编辑表单里的时间草稿与后端 `schedule` 字符串之间的转换。
 * 常用方式在前端生成同样的字符串（cron / every / 本地时间），高级方式直接编辑字符串。
 */
import { format } from "date-fns";

export type ScheduleMode = "once" | "daily" | "weekly" | "monthly" | "interval" | "advanced";
export type IntervalUnit = "m" | "h" | "d";

export interface ScheduleDraft {
    mode: ScheduleMode;
    /** yyyy-MM-dd（一次性） */
    date: string;
    /** HH:mm */
    time: string;
    /** 0=周日 … 6=周六 */
    weekDays: number[];
    /** 1-31 */
    monthDays: number[];
    intervalValue: string;
    intervalUnit: IntervalUnit;
    advanced: string;
}

export const scheduleModeLabels: Record<ScheduleMode, string> = {
    once: "一次",
    daily: "每天",
    weekly: "每周",
    monthly: "每月",
    interval: "间隔",
    advanced: "高级",
};

export const intervalUnitLabels: Record<IntervalUnit, string> = { m: "分钟", h: "小时", d: "天" };

/** 周一在前的展示顺序。 */
export const WEEK_DAY_ORDER = [1, 2, 3, 4, 5, 6, 0];
export const weekDayShortLabels: Record<number, string> = { 0: "日", 1: "一", 2: "二", 3: "三", 4: "四", 5: "五", 6: "六" };

export function defaultScheduleDraft(now: Date = new Date()): ScheduleDraft {
    const nextHour = new Date(now.getTime() + 60 * 60 * 1000);
    return {
        mode: "daily",
        date: format(nextHour, "yyyy-MM-dd"),
        time: "09:00",
        weekDays: [now.getDay()],
        monthDays: [now.getDate()],
        intervalValue: "1",
        intervalUnit: "h",
        advanced: "",
    };
}

function splitTime(time: string): [number, number] | null {
    const match = /^(\d{1,2}):(\d{2})$/.exec(time.trim());
    if (!match) return null;
    const hour = Number(match[1]);
    const minute = Number(match[2]);
    return hour < 24 && minute < 60 ? [hour, minute] : null;
}

const sortedUnique = (values: number[]) => [...new Set(values)].sort((a, b) => a - b);

/** 草稿 → schedule 字符串；信息不全时返回 null。 */
export function buildSchedule(draft: ScheduleDraft): string | null {
    const time = splitTime(draft.time);
    switch (draft.mode) {
        case "once":
            return /^\d{4}-\d{2}-\d{2}$/.test(draft.date) && time ? `${draft.date} ${draft.time.padStart(5, "0")}` : null;
        case "daily":
            return time ? `${time[1]} ${time[0]} * * *` : null;
        case "weekly":
            return time && draft.weekDays.length ? `${time[1]} ${time[0]} * * ${sortedUnique(draft.weekDays).join(",")}` : null;
        case "monthly":
            return time && draft.monthDays.length ? `${time[1]} ${time[0]} ${sortedUnique(draft.monthDays).join(",")} * *` : null;
        case "interval": {
            const value = Number(draft.intervalValue);
            return Number.isInteger(value) && value > 0 ? `every ${value}${draft.intervalUnit}` : null;
        }
        case "advanced":
            return draft.advanced.trim() || null;
    }
}

const numberList = (field: string, min: number, max: number): number[] | null => {
    const values = field.split(",").map((part) => (/^\d+$/.test(part) ? Number(part) : NaN));
    return values.every((v) => Number.isInteger(v) && v >= min && v <= max) ? sortedUnique(values) : null;
};

const pad = (value: number) => String(value).padStart(2, "0");

/** schedule 字符串 → 草稿；认不出的写法放进高级方式原样编辑。 */
export function draftFromSchedule(schedule: string, now: Date = new Date()): ScheduleDraft {
    const draft = defaultScheduleDraft(now);
    const text = schedule.trim();
    const advanced = { ...draft, mode: "advanced" as const, advanced: text };
    const every = /^every (\d+)([mhd])$/.exec(text);
    if (every) {
        return { ...draft, mode: "interval", intervalValue: every[1], intervalUnit: every[2] as IntervalUnit };
    }
    const iso = /^(\d{4}-\d{2}-\d{2})[T ](\d{2}:\d{2})/.exec(text);
    if (iso) {
        const date = new Date(text.includes("+") || text.endsWith("Z") ? text : `${iso[1]}T${iso[2]}`);
        if (!Number.isNaN(date.getTime())) {
            return { ...draft, mode: "once", date: format(date, "yyyy-MM-dd"), time: format(date, "HH:mm") };
        }
    }
    const fields = text.split(/\s+/);
    if (fields.length !== 5 || !/^\d+$/.test(fields[0]) || !/^\d+$/.test(fields[1]) || fields[3] !== "*") {
        return advanced;
    }
    const minute = Number(fields[0]);
    const hour = Number(fields[1]);
    if (minute > 59 || hour > 23) return advanced;
    const time = `${pad(hour)}:${pad(minute)}`;
    const [, , day, , weekday] = fields;
    if (day === "*" && weekday === "*") return { ...draft, mode: "daily", time };
    if (day === "*") {
        const weekDays = numberList(weekday, 0, 6);
        return weekDays ? { ...draft, mode: "weekly", time, weekDays } : advanced;
    }
    if (weekday === "*") {
        const monthDays = numberList(day, 1, 31);
        return monthDays ? { ...draft, mode: "monthly", time, monthDays } : advanced;
    }
    return advanced;
}
