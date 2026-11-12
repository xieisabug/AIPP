import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronDown, ChevronRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import {
    WEEK_DAY_ORDER,
    buildSchedule,
    defaultScheduleDraft,
    draftFromSchedule,
    intervalUnitLabels,
    scheduleModeLabels,
    weekDayShortLabels,
    type IntervalUnit,
    type ScheduleDraft,
    type ScheduleMode,
} from "./scheduleEditor";
import { deliverPolicyLabels } from "./scheduleFormat";
import type {
    DeliverPolicy,
    DeliverTo,
    MissedPolicy,
    SchedulePreview,
    ScheduledTask,
    ScheduledTaskInput,
} from "./scheduleTypes";

export interface AssistantOption {
    id: number;
    name: string;
}

export interface ScheduleTaskEditorProps {
    open: boolean;
    /** 编辑已有任务；不传为新建。 */
    task?: ScheduledTask | null;
    assistants: AssistantOption[];
    saving: boolean;
    onOpenChange: (open: boolean) => void;
    onSubmit: (input: ScheduledTaskInput) => void;
}

interface FormState {
    name: string;
    assistantId: string;
    draft: ScheduleDraft;
    prompt: string;
    deliverTo: DeliverTo;
    deliverPolicy: DeliverPolicy;
    endsAt: string;
    maxRuns: string;
    missedPolicy: MissedPolicy;
    enabled: boolean;
}

const PREVIEW_DEBOUNCE_MS = 300;

function initialState(task: ScheduledTask | null | undefined, assistants: AssistantOption[]): FormState {
    if (!task) {
        return {
            name: "",
            assistantId: assistants[0] ? String(assistants[0].id) : "",
            draft: defaultScheduleDraft(),
            prompt: "",
            deliverTo: "owner_chat",
            deliverPolicy: "always",
            endsAt: "",
            maxRuns: "",
            missedPolicy: "run_once",
            enabled: true,
        };
    }
    return {
        name: task.name,
        assistantId: String(task.assistantId),
        draft: draftFromSchedule(task.schedule),
        prompt: task.prompt,
        deliverTo: task.deliverTo,
        deliverPolicy: task.deliverPolicy,
        endsAt: task.endsAt ? task.endsAt.slice(0, 10) : "",
        maxRuns: task.maxRuns ? String(task.maxRuns) : "",
        missedPolicy: task.missedPolicy,
        enabled: task.enabled,
    };
}

function FieldLabel({ children, hint }: { children: React.ReactNode; hint?: string }) {
    return (
        <div className="flex items-baseline justify-between gap-2">
            <Label className="text-xs font-medium">{children}</Label>
            {hint && <span className="text-[11px] text-muted-foreground">{hint}</span>}
        </div>
    );
}

function ScheduleFields({ draft, onChange }: { draft: ScheduleDraft; onChange: (draft: ScheduleDraft) => void }) {
    const set = (patch: Partial<ScheduleDraft>) => onChange({ ...draft, ...patch });
    const timeInput = (
        <Input type="time" value={draft.time} onChange={(e) => set({ time: e.target.value })} className="h-8 w-28 text-sm" aria-label="时刻" />
    );
    return (
        <div className="space-y-3">
            <ToggleGroup
                type="single"
                variant="outline"
                size="sm"
                value={draft.mode}
                onValueChange={(value) => value && set({ mode: value as ScheduleMode })}
                className="flex flex-wrap justify-start"
                aria-label="重复方式"
            >
                {(Object.keys(scheduleModeLabels) as ScheduleMode[]).map((mode) => (
                    <ToggleGroupItem key={mode} value={mode} className="px-3 text-xs">
                        {scheduleModeLabels[mode]}
                    </ToggleGroupItem>
                ))}
            </ToggleGroup>
            {draft.mode === "once" && (
                <div className="flex flex-wrap items-center gap-2">
                    <Input type="date" value={draft.date} onChange={(e) => set({ date: e.target.value })} className="h-8 w-40 text-sm" aria-label="日期" />
                    {timeInput}
                </div>
            )}
            {draft.mode === "daily" && <div className="flex items-center gap-2 text-sm">每天 {timeInput}</div>}
            {draft.mode === "weekly" && (
                <div className="space-y-2">
                    <ToggleGroup
                        type="multiple"
                        variant="outline"
                        size="sm"
                        value={draft.weekDays.map(String)}
                        onValueChange={(values) => set({ weekDays: values.map(Number) })}
                        className="flex flex-wrap justify-start"
                        aria-label="星期"
                    >
                        {WEEK_DAY_ORDER.map((day) => (
                            <ToggleGroupItem key={day} value={String(day)} className="w-9 text-xs" aria-label={`周${weekDayShortLabels[day]}`}>
                                {weekDayShortLabels[day]}
                            </ToggleGroupItem>
                        ))}
                    </ToggleGroup>
                    <div className="flex items-center gap-2 text-sm">时刻 {timeInput}</div>
                </div>
            )}
            {draft.mode === "monthly" && (
                <div className="space-y-2">
                    <ToggleGroup
                        type="multiple"
                        variant="outline"
                        size="sm"
                        value={draft.monthDays.map(String)}
                        onValueChange={(values) => set({ monthDays: values.map(Number) })}
                        className="grid grid-cols-7 gap-1"
                        aria-label="日期"
                    >
                        {Array.from({ length: 31 }, (_, i) => i + 1).map((day) => (
                            <ToggleGroupItem key={day} value={String(day)} className="h-7 w-full px-0 text-xs">
                                {day}
                            </ToggleGroupItem>
                        ))}
                    </ToggleGroup>
                    <div className="flex items-center gap-2 text-sm">时刻 {timeInput}</div>
                </div>
            )}
            {draft.mode === "interval" && (
                <div className="flex items-center gap-2 text-sm">
                    每
                    <Input
                        type="number"
                        min={1}
                        value={draft.intervalValue}
                        onChange={(e) => set({ intervalValue: e.target.value })}
                        className="h-8 w-20 text-sm"
                        aria-label="间隔数值"
                    />
                    <Select value={draft.intervalUnit} onValueChange={(value) => set({ intervalUnit: value as IntervalUnit })}>
                        <SelectTrigger className="h-8 w-24 text-sm" aria-label="间隔单位">
                            <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                            {(Object.keys(intervalUnitLabels) as IntervalUnit[]).map((unit) => (
                                <SelectItem key={unit} value={unit}>
                                    {intervalUnitLabels[unit]}
                                </SelectItem>
                            ))}
                        </SelectContent>
                    </Select>
                </div>
            )}
            {draft.mode === "advanced" && (
                <div className="space-y-1">
                    <Input
                        value={draft.advanced}
                        onChange={(e) => set({ advanced: e.target.value })}
                        placeholder="例如 0 9 * * 1-5、every 2h、every 1w at 16:00、2026-12-01 09:00"
                        className="h-8 font-mono text-sm"
                        aria-label="时间表达"
                    />
                    <p className="text-[11px] leading-4 text-muted-foreground">
                        支持 5 段 cron（分 时 日 月 周，按本机时间）、every 间隔、具体时间或 in 30m 这样的相对时间。
                    </p>
                </div>
            )}
        </div>
    );
}

export default function ScheduleTaskEditor({ open, task, assistants, saving, onOpenChange, onSubmit }: ScheduleTaskEditorProps) {
    const [form, setForm] = useState<FormState>(() => initialState(task, assistants));
    const [preview, setPreview] = useState<SchedulePreview | null>(null);
    const [previewError, setPreviewError] = useState<string | null>(null);
    const [showMore, setShowMore] = useState(false);

    useEffect(() => {
        if (open) {
            setForm(initialState(task, assistants));
            setShowMore(Boolean(task?.endsAt || task?.maxRuns || task?.missedPolicy === "skip"));
        }
        // 只在打开或切换任务时重置；助手列表刷新不覆盖正在编辑的内容
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [open, task?.id]);

    const schedule = useMemo(() => buildSchedule(form.draft), [form.draft]);

    useEffect(() => {
        if (!open) return;
        if (!schedule) {
            setPreview(null);
            setPreviewError(null);
            return;
        }
        let cancelled = false;
        const timer = window.setTimeout(() => {
            invoke<SchedulePreview>("preview_schedule", { schedule })
                .then((result) => {
                    if (cancelled) return;
                    setPreview(result);
                    setPreviewError(null);
                })
                .catch((error) => {
                    if (cancelled) return;
                    setPreview(null);
                    setPreviewError(String(error));
                });
        }, PREVIEW_DEBOUNCE_MS);
        return () => {
            cancelled = true;
            window.clearTimeout(timer);
        };
    }, [open, schedule]);

    const set = (patch: Partial<FormState>) => setForm((prev) => ({ ...prev, ...patch }));
    const keepsConversation = task?.deliverTo === "conversation";
    const canSubmit = Boolean(form.name.trim() && form.assistantId && form.prompt.trim() && schedule && preview && !saving);

    const submit = () => {
        if (!canSubmit || !schedule) return;
        const maxRuns = Number(form.maxRuns);
        onSubmit({
            name: form.name.trim(),
            schedule,
            prompt: form.prompt.trim(),
            assistantId: Number(form.assistantId),
            deliverPolicy: form.deliverPolicy,
            deliverTo: form.deliverTo,
            endsAt: form.endsAt ? `${form.endsAt} 23:59` : task ? "" : null,
            maxRuns: Number.isInteger(maxRuns) && maxRuns > 0 ? maxRuns : task ? 0 : null,
            missedPolicy: form.missedPolicy,
            enabled: form.enabled,
        });
    };

    return (
        <Dialog open={open} onOpenChange={onOpenChange}>
            <DialogContent className="sm:max-w-[640px] max-h-[88vh] overflow-y-auto" data-aipp-slot="schedule-editor-dialog">
                <DialogHeader>
                    <DialogTitle className="text-base">{task ? "编辑定时任务" : "新建定时任务"}</DialogTitle>
                    <DialogDescription className="text-xs">
                        每次运行都是一个全新的会话，助手看不到之前的聊天；需要的背景和要求请都写进指令里。
                    </DialogDescription>
                </DialogHeader>

                <div className="space-y-5 py-1">
                    <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                        <div className="space-y-1.5">
                            <FieldLabel>名称</FieldLabel>
                            <Input
                                value={form.name}
                                onChange={(e) => set({ name: e.target.value })}
                                placeholder="例如：周四休息提醒"
                                className="h-8 text-sm"
                                aria-label="任务名称"
                            />
                        </div>
                        <div className="space-y-1.5">
                            <FieldLabel>执行助手</FieldLabel>
                            <Select value={form.assistantId} onValueChange={(value) => set({ assistantId: value })}>
                                <SelectTrigger className="h-8 text-sm" aria-label="执行助手">
                                    <SelectValue placeholder="选择普通对话助手" />
                                </SelectTrigger>
                                <SelectContent>
                                    {assistants.map((assistant) => (
                                        <SelectItem key={assistant.id} value={String(assistant.id)}>
                                            {assistant.name}
                                        </SelectItem>
                                    ))}
                                </SelectContent>
                            </Select>
                        </div>
                    </div>

                    <div className="space-y-2">
                        <FieldLabel>什么时候</FieldLabel>
                        <ScheduleFields draft={form.draft} onChange={(draft) => set({ draft })} />
                        <div className="rounded-md bg-muted/40 px-3 py-2 text-xs" aria-live="polite" data-aipp-slot="schedule-editor-preview">
                            {previewError ? (
                                <span className="text-destructive">{previewError}</span>
                            ) : preview ? (
                                <div className="space-y-1">
                                    <div className="font-medium text-foreground">{preview.scheduleText}</div>
                                    <div className="text-muted-foreground">接下来：{preview.upcomingText.join("、")}</div>
                                </div>
                            ) : (
                                <span className="text-muted-foreground">把时间填完整后，这里会显示接下来几次的运行时间</span>
                            )}
                        </div>
                    </div>

                    <div className="space-y-1.5">
                        <FieldLabel hint="运行时用户不在场">要做什么</FieldLabel>
                        <Textarea
                            value={form.prompt}
                            onChange={(e) => set({ prompt: e.target.value })}
                            placeholder="写清背景、偏好、要做的事和输出要求；检查类任务再写清楚什么情况值得告诉你。"
                            className="min-h-32 text-sm"
                            aria-label="任务指令"
                        />
                    </div>

                    <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                        <div className="space-y-1.5">
                            <FieldLabel>结果发到</FieldLabel>
                            <Select value={form.deliverTo} onValueChange={(value) => set({ deliverTo: value as DeliverTo })}>
                                <SelectTrigger className="h-8 text-sm" aria-label="结果发到">
                                    <SelectValue />
                                </SelectTrigger>
                                <SelectContent>
                                    <SelectItem value="owner_chat">执行助手的管家私聊</SelectItem>
                                    <SelectItem value="run_conversation">单独的会话（出现在聊天列表）</SelectItem>
                                    {keepsConversation && <SelectItem value="conversation">{task?.deliverTargetText}</SelectItem>}
                                </SelectContent>
                            </Select>
                        </div>
                        <div className="space-y-1.5">
                            <FieldLabel>什么时候发</FieldLabel>
                            <Select value={form.deliverPolicy} onValueChange={(value) => set({ deliverPolicy: value as DeliverPolicy })}>
                                <SelectTrigger className="h-8 text-sm" aria-label="什么时候发">
                                    <SelectValue />
                                </SelectTrigger>
                                <SelectContent>
                                    {(Object.keys(deliverPolicyLabels) as DeliverPolicy[]).map((policy) => (
                                        <SelectItem key={policy} value={policy}>
                                            {deliverPolicyLabels[policy].label}
                                        </SelectItem>
                                    ))}
                                </SelectContent>
                            </Select>
                            <p className="text-[11px] text-muted-foreground">{deliverPolicyLabels[form.deliverPolicy].description}</p>
                        </div>
                    </div>

                    <div className="space-y-3">
                        <button
                            type="button"
                            className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
                            onClick={() => setShowMore((prev) => !prev)}
                            aria-expanded={showMore}
                        >
                            {showMore ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
                            结束条件与错过处理
                        </button>
                        {showMore && (
                            <div className="grid grid-cols-1 gap-3 rounded-md border border-border p-3 md:grid-cols-3">
                                <div className="space-y-1.5">
                                    <FieldLabel>截止日期</FieldLabel>
                                    <Input type="date" value={form.endsAt} onChange={(e) => set({ endsAt: e.target.value })} className="h-8 text-sm" aria-label="截止日期" />
                                </div>
                                <div className="space-y-1.5">
                                    <FieldLabel>最多运行</FieldLabel>
                                    <Input
                                        type="number"
                                        min={1}
                                        value={form.maxRuns}
                                        onChange={(e) => set({ maxRuns: e.target.value })}
                                        placeholder="不限"
                                        className="h-8 text-sm"
                                        aria-label="最多运行次数"
                                    />
                                </div>
                                <div className="space-y-1.5">
                                    <FieldLabel>应用没开时</FieldLabel>
                                    <Select value={form.missedPolicy} onValueChange={(value) => set({ missedPolicy: value as MissedPolicy })}>
                                        <SelectTrigger className="h-8 text-sm" aria-label="错过处理">
                                            <SelectValue />
                                        </SelectTrigger>
                                        <SelectContent>
                                            <SelectItem value="run_once">补跑最近一次</SelectItem>
                                            <SelectItem value="skip">只记录，不补跑</SelectItem>
                                        </SelectContent>
                                    </Select>
                                </div>
                            </div>
                        )}
                    </div>

                    <label className="flex items-center gap-2 text-xs">
                        <Switch checked={form.enabled} onCheckedChange={(enabled) => set({ enabled })} aria-label="启用任务" />
                        保存后立即启用
                    </label>
                </div>

                <DialogFooter>
                    <Button variant="outline" size="sm" onClick={() => onOpenChange(false)}>
                        取消
                    </Button>
                    <Button size="sm" onClick={submit} disabled={!canSubmit}>
                        {saving ? "保存中…" : "保存"}
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}
