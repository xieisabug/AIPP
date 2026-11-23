import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CalendarHeart } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
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

const cadences = {
    gentle: { label: "温和", pulse_hours: 3, daily_limit: 3 },
    active: { label: "积极", pulse_hours: 1, daily_limit: 6 },
} as const;
type Cadence = keyof typeof cadences | "custom";

interface AssistantOption {
    id: number;
    name: string;
    assistant_type?: number | null;
}

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
    const [nativeAgents, setNativeAgents] = useState<AssistantOption[]>([]);
    const [suggestion, setSuggestion] = useState<Center["window"]>();
    const [customCadence, setCustomCadence] = useState(false);
    const enabled = value?.enabled === true;

    useEffect(() => {
        if (!enabled) return;
        let current = true;
        // 建议时段与原生 Agent 列表只是辅助信息，读取失败不打扰设置本身
        center().then(c => { if (current) setSuggestion(c.window); }).catch(() => undefined);
        invoke<AssistantOption[]>("get_assistants")
            .then(list => {
                if (!current || !Array.isArray(list)) return;
                setNativeAgents(list.filter(a => a.assistant_type === 4));
            })
            .catch(() => undefined);
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

    // 只有建议与当前时间不同、且没开自动调整时，才提供一键采用
    const adoptable = !value.adaptive_window && suggestion
        && (suggestion.release !== value.release || suggestion.digest !== value.digest) ? suggestion : undefined;

    return <>
        <SettingsGroup
            title="主动整理与关怀"
            description="管家在后台空闲时自主回顾聊天、按需调用工具整理记忆或准备资料，并在合适的时候主动提醒。所有工具调用均经过安全审核门禁，未通过的动作会转为建议卡片交由你确认。"
            action={enabled ? <Button type="button" variant="outline" size="sm" onClick={onOpenCenter}
                data-aipp-slot="butler-proactive-open-center">
                <CalendarHeart className="h-4 w-4 mr-1.5" />跟进事项与整理手记
            </Button> : null}
        >
            <Row label="启用主动整理" description={value.release_pending
                ? "本机已停止整理，跨设备执行权仍待释放。同步完成后点击「保存配置」重试。"
                : "默认关闭。使用同步服务时，一个账户只由一台设备负责整理；换设备前先在原设备关闭。"}>
                <Switch aria-label="启用主动整理" checked={enabled} onCheckedChange={v => set("enabled", v)} />
            </Row>
            {enabled && <>
                <div className={stackedRowClassName}>
                    <SettingRowText label="整理范围" description="管家可以回顾哪些聊天。普通聊天需要单独勾选。" />
                    <CheckboxGroup items={scopes} value={value} onToggle={set} />
                </div>
                <Row
                    label="主动提醒与推送"
                    description="开启后按节律主动发送桌面提醒；若总管家已开启飞书，高优先级提醒会自动由总管家同步到飞书。关闭后仅在后台静默整理。"
                >
                    <Switch
                        aria-label="主动提醒与推送"
                        checked={value.allow_notifications}
                        onCheckedChange={v => set("allow_notifications", v)}
                    />
                </Row>
                <div className={stackedRowClassName}>
                    <SettingRowText
                        label="整理偏好与关注重点"
                        description="可选。用自然语言告诉管家平时重点关注什么、希望怎么整理；留空则由管家自主判断。"
                    />
                    <Textarea
                        aria-label="整理偏好与关注重点"
                        rows={3}
                        maxLength={4000}
                        placeholder="例如：重点关注项目截止日期、未跟进承诺与日程变更；日常闲聊不要打扰。"
                        value={value.custom_instructions}
                        onChange={e => set("custom_instructions", e.target.value)}
                    />
                </div>
                <Row label="初筛模型" description="聊后与定期巡检时判断有没有值得介入的新动态，适合用小模型。">
                    <Select value={value.small_model || INHERIT} onValueChange={v => set("small_model", v === INHERIT ? "" : v)}>
                        <SelectTrigger aria-label="初筛模型" className="w-64"><SelectValue /></SelectTrigger>
                        <SelectContent>
                            <SelectItem value={INHERIT}>沿用管家辅助模型</SelectItem>
                            {models.map(m => <SelectItem key={m.id} value={`${m.code}%%${m.llm_provider_id}`}>{m.name}</SelectItem>)}
                        </SelectContent>
                    </Select>
                </Row>
                <Row label="整理执行者" description="负责深度复盘、资料调研、整理工作区与撰写提醒。支持普通模型或原生 Agent 助手。">
                    <Select value={value.deep_model || INHERIT} onValueChange={v => set("deep_model", v === INHERIT ? "" : v)}>
                        <SelectTrigger aria-label="整理执行者" className="w-64"><SelectValue /></SelectTrigger>
                        <SelectContent>
                            <SelectItem value={INHERIT}>沿用总管家模型</SelectItem>
                            {nativeAgents.length > 0 && <SelectGroup>
                                <SelectLabel>原生 Agent 助手</SelectLabel>
                                {nativeAgents.map(a => (
                                    <SelectItem key={`assistant-${a.id}`} value={`assistant:${a.id}`}>
                                        {a.name}（原生 Agent）
                                    </SelectItem>
                                ))}
                            </SelectGroup>}
                            {models.length > 0 && <SelectGroup>
                                <SelectLabel>普通模型</SelectLabel>
                                {models.map(m => (
                                    <SelectItem key={m.id} value={`${m.code}%%${m.llm_provider_id}`}>{m.name}</SelectItem>
                                ))}
                            </SelectGroup>}
                        </SelectContent>
                    </Select>
                </Row>
            </>}
        </SettingsGroup>

        {enabled && <SettingsGroup
            title="提醒节律与送达"
            description="总管家开启飞书通道后，高优先级提醒会自动由总管家发往飞书，无需单独配置飞书接收人。"
        >
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
        </SettingsGroup>}
    </>;
}
