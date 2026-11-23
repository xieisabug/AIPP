import { useEffect, useState, type ReactNode } from "react";
import { CalendarHeart } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { SettingRowText, SettingsGroup, settingRowClassName, stackedRowClassName } from "@/components/config/SettingsGroup";
import { useFilteredModels } from "@/hooks/useFilteredModels";
import { center, type Center, type ProactiveConfig } from "./proactiveApi";

// Radix Select 不允许空字符串作为选项值，用哨兵值表示「沿用默认」。
const INHERIT = "__inherit__";

const scopes = [
    ["private_chats", "总管家与联系人私聊"],
    ["groups", "管家群聊"],
    ["normal_chats", "普通聊天"],
] as const;
const abilities = [
    ["remember", "整理长期记忆"],
    ["research", "提前查资料、准备简报"],
    ["reminders", "主动提醒与祝福"],
] as const;
const cadences = {
    gentle: { label: "温和", pulse_hours: 3, daily_limit: 3 },
    active: { label: "积极", pulse_hours: 1, daily_limit: 6 },
} as const;
type Cadence = keyof typeof cadences | "custom";

const presetOf = (c: ProactiveConfig): Cadence =>
    (Object.keys(cadences) as (keyof typeof cadences)[]).find(
        key => cadences[key].pulse_hours === c.pulse_hours && cadences[key].daily_limit === c.daily_limit,
    ) ?? "custom";

function CheckboxGroup<K extends keyof ProactiveConfig>({ items, value, onToggle }: {
    items: readonly (readonly [K, string])[];
    value: ProactiveConfig;
    onToggle: (key: K, checked: boolean) => void;
}) {
    return <div className="flex flex-wrap gap-x-6 gap-y-2">
        {items.map(([key, label]) => <label key={key} className="flex cursor-pointer items-center gap-2 text-sm">
            <Checkbox checked={value[key] === true} onCheckedChange={checked => onToggle(key, checked === true)} />
            {label}
        </label>)}
    </div>;
}

const TimeInput = ({ label, value, onChange }: { label: string; value: string; onChange: (v: string) => void }) =>
    <Input type="time" aria-label={label} className="w-32" value={value} onChange={e => onChange(e.target.value)} />;

const Row = ({ label, description, children }: { label: ReactNode; description?: ReactNode; children: ReactNode }) =>
    <div className={settingRowClassName}>
        <SettingRowText label={label} description={description} />
        <div className="flex shrink-0 items-center gap-2">{children}</div>
    </div>;

/**
 * 总管家设置弹窗里的「主动整理与关怀」分组。只放配置项，由弹窗底部统一的「保存配置」保存；
 * 跟进事项、整理手记和「立即整理一次」属于运行内容，在 ButlerProactiveCenter 里。
 */
export default function ButlerProactiveSettings({ value, error, onChange, onOpenCenter }: {
    value?: ProactiveConfig;
    error?: string;
    onChange: (next: ProactiveConfig) => void;
    onOpenCenter: () => void;
}) {
    const { models } = useFilteredModels(0);
    const [suggestion, setSuggestion] = useState<Center["window"]>();
    const [customCadence, setCustomCadence] = useState(false);
    const enabled = value?.enabled === true;

    useEffect(() => {
        if (!enabled) return;
        let current = true;
        // 建议时段只是辅助信息，读取失败不打扰设置本身
        center().then(c => { if (current) setSuggestion(c.window); }).catch(() => undefined);
        return () => { current = false; };
    }, [enabled]);

    if (!value) {
        return <SettingsGroup title="主动整理与关怀">
            <div className={settingRowClassName}>
                <SettingRowText label="主动整理" description={error || "正在读取设置…"} />
            </div>
        </SettingsGroup>;
    }

    const set = <K extends keyof ProactiveConfig>(key: K, next: ProactiveConfig[K]) => onChange({ ...value, [key]: next });
    const cadence: Cadence = customCadence ? "custom" : presetOf(value);
    const modelSelect = (key: "small_model" | "deep_model", inheritLabel: string) =>
        <Select value={value[key] || INHERIT} onValueChange={v => set(key, v === INHERIT ? "" : v)}>
            <SelectTrigger className="w-64"><SelectValue /></SelectTrigger>
            <SelectContent>
                <SelectItem value={INHERIT}>{inheritLabel}</SelectItem>
                {models.map(m => <SelectItem key={m.id} value={`${m.code}%%${m.llm_provider_id}`}>{m.name}</SelectItem>)}
            </SelectContent>
        </Select>;
    // 只有建议与当前时间不同、且没开自动调整时，才提供一键采用
    const adoptable = !value.adaptive_window && suggestion
        && (suggestion.release !== value.release || suggestion.digest !== value.digest) ? suggestion : undefined;

    return <>
        <SettingsGroup
            title="主动整理与关怀"
            description="管家在后台回顾聊天，整理长期记忆、提前准备资料，并在合适的时候主动提醒。"
            action={enabled ? <Button type="button" variant="outline" size="sm" onClick={onOpenCenter}
                data-aipp-slot="butler-proactive-open-center">
                <CalendarHeart className="h-4 w-4 mr-1.5" />跟进事项与整理手记
            </Button> : null}
        >
            <Row label="启用主动整理" description="默认关闭。使用同步服务时，一个账户只由一台设备负责整理；换设备前先在原设备关闭。">
                <Switch aria-label="启用主动整理" checked={enabled} onCheckedChange={v => set("enabled", v)} />
            </Row>
            {enabled && <>
                <div className={stackedRowClassName}>
                    <SettingRowText label="整理范围" description="管家可以回顾哪些聊天。普通聊天需要单独勾选。" />
                    <CheckboxGroup items={scopes} value={value} onToggle={set} />
                </div>
                <div className={stackedRowClassName}>
                    <SettingRowText label="可以做的事" description="预研只做只读操作；需要执行的动作会以建议卡片交给你确认。" />
                    <CheckboxGroup items={abilities} value={value} onToggle={set} />
                </div>
                <Row label="初筛模型" description="判断近期聊天有没有值得跟进的新事项，适合用小模型。">
                    {modelSelect("small_model", "沿用管家辅助模型")}
                </Row>
                <Row label="整理模型" description="复盘、预研和撰写提醒。">
                    {modelSelect("deep_model", "沿用总管家模型")}
                </Row>
            </>}
        </SettingsGroup>

        {enabled && <SettingsGroup title="提醒节律与送达">
            <div className={stackedRowClassName}>
                <div className="flex items-center justify-between gap-4">
                    <SettingRowText label="提醒节律"
                        description={`${cadence === "custom" ? "" : `每 ${value.pulse_hours} 小时巡检一次，每天最多 ${value.daily_limit} 条提醒。`}连续两条无人阅读或回应时暂停推送，后台整理照常进行。`} />
                    <Select value={cadence} onValueChange={next => {
                        const preset = next as Cadence;
                        setCustomCadence(preset === "custom");
                        if (preset !== "custom") onChange({ ...value, pulse_hours: cadences[preset].pulse_hours, daily_limit: cadences[preset].daily_limit });
                    }}>
                        <SelectTrigger className="w-64"><SelectValue /></SelectTrigger>
                        <SelectContent>
                            {(Object.keys(cadences) as (keyof typeof cadences)[]).map(key =>
                                <SelectItem key={key} value={key}>{cadences[key].label}</SelectItem>)}
                            <SelectItem value="custom">自定义</SelectItem>
                        </SelectContent>
                    </Select>
                </div>
                {cadence === "custom" && <div className="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
                    每<Input type="number" aria-label="巡检间隔（小时）" className="h-8 w-16" min={1} max={24}
                        value={value.pulse_hours} onChange={e => set("pulse_hours", Number(e.target.value))} />小时巡检一次，每天最多
                    <Input type="number" aria-label="每日触达上限" className="h-8 w-16" min={0} max={100}
                        value={value.daily_limit} onChange={e => set("daily_limit", Number(e.target.value))} />条提醒
                </div>}
            </div>
            <Row label="免打扰时段" description="期间不推送，但可以完成准备。">
                <TimeInput label="免打扰开始" value={value.quiet_start} onChange={v => set("quiet_start", v)} />
                <span className="text-sm text-muted-foreground">至</span>
                <TimeInput label="免打扰结束" value={value.quiet_end} onChange={v => set("quiet_end", v)} />
            </Row>
            <Row label="夜间提醒送达" description="免打扰期间产生的提醒在这个时间后按联系人合并送达，须晚于免打扰结束。">
                <TimeInput label="夜间提醒送达" value={value.release} onChange={v => set("release", v)} />
            </Row>
            <Row label="每日复盘" description="用当天的聊天做一次深度复盘；错过的复盘会在电脑恢复空闲后补做。">
                <TimeInput label="每日复盘" value={value.digest} onChange={v => set("digest", v)} />
            </Row>
            <Row label="按近两周活跃时间调整"
                description={suggestion
                    ? `近 ${suggestion.days} 天的活跃时段建议 ${suggestion.release} 送达、${suggestion.digest} 复盘。开启后自动采用，活跃数据不足时沿用上面的时间。`
                    : "开启后按最近的聊天时段自动调整送达和复盘时间，活跃数据不足时沿用上面的时间。"}>
                {adoptable && <Button type="button" variant="outline" size="sm"
                    onClick={() => onChange({ ...value, release: adoptable.release, digest: adoptable.digest })}>采用建议时间</Button>}
                <Switch aria-label="按近两周活跃时间调整" checked={value.adaptive_window} onCheckedChange={v => set("adaptive_window", v)} />
            </Row>
            <Row label="飞书提醒接收人" description="填写飞书用户 Open ID（ou_ 开头），只推送高优先级事项的简短提示；留空则不推送。">
                <Input className="w-64" placeholder="ou_xxx" aria-label="飞书提醒接收人"
                    value={value.feishu_open_id} onChange={e => set("feishu_open_id", e.target.value.trim())} />
            </Row>
        </SettingsGroup>}
    </>;
}
