import { useEffect, useState, type ReactNode } from "react";
import { Check, X } from "lucide-react";
import { useReducedMotion } from "motion/react";
import { accessoriesOf, accessoryDefinitions, avatarDefinitions, type AccessoryId, type AvatarAction } from "@/components/avatar/avatarDefinition";
import { accessoryPreviewViewBox, BlushArtwork, BowBellArtwork, GlassesArtwork, HatArtwork, FlowerArtwork, SunglassesArtwork } from "@/components/avatar/AvatarAccessories";
import { Button } from "@/components/ui/button";
import ButlerAvatar from "./ButlerAvatar";
import { defaultAppearance, type ButlerAppearance } from "./butlerAppearance";
import { accessoryActionsOf, showcaseDelay, showcaseStep } from "./avatarShowcase";

const lightColors = [
    ["纯白", "#FFFFFF"], ["奶油黄", "#FFF0B5"], ["蜜桃", "#FFD6C2"],
    ["樱花粉", "#FAD4E4"], ["浅紫", "#E4DAFF"], ["浅蓝", "#CDE9FF"],
    ["薄荷绿", "#CEF3DF"], ["青柠", "#E5F4B8"],
];
const darkColors = [
    ["纯黑", "#000000"], ["石墨", "#343B48"], ["海军蓝", "#193B68"],
    ["靛蓝", "#39316D"], ["深紫", "#613C72"], ["酒红", "#793D50"],
    ["墨绿", "#205448"], ["深棕", "#624735"],
];

const accessorySlots = [["eyes", "脸部"], ["head", "头部"]] as const;

/** 新增与编辑助手对话框共用的固定尺寸：高度固定，切换页面或内容多少都不改变大小。 */
export const PROFILE_DIALOG_CLASS = "flex h-[min(820px,90vh)] flex-col overflow-hidden sm:max-w-[760px]";
/** 底部操作栏：贴在对话框底部，不随表单滚动。 */
export const PROFILE_FOOTER_CLASS = "-mx-6 shrink-0 border-t border-border px-6 pt-4";

function AccessoryIcon({ id, appearance }: { id: AccessoryId; appearance: ButlerAppearance }) {
    const artwork = {
        "round-glasses": <GlassesArtwork />, sunglasses: <SunglassesArtwork />, blush: <BlushArtwork />, beret: <HatArtwork />,
        "bow-bell": <BowBellArtwork color={appearance.colors.primary} outline={appearance.colors.secondary} />, flower: <FlowerArtwork />,
    }[id];
    return <svg aria-hidden="true" viewBox={accessoryPreviewViewBox[id]} className="size-8">{artwork}</svg>;
}

interface Props {
    /** 预览下方显示的名字。 */
    name: string;
    appearance: ButlerAppearance;
    onAppearanceChange: (next: ButlerAppearance) => void;
    disabled?: boolean;
    /** 形象之前的基本信息字段（名称、描述、类型、模型）；只编辑头像时不传。 */
    children?: ReactNode;
}

/**
 * 助手资料编辑区：左侧形象预览（垂直居中、不随表单滚动），右侧表单单独滚动。
 * 放在固定高度的 flex 容器里使用；窄屏时两栏上下排列，整体滚动。
 */
export default function AssistantProfileEditor({ name, appearance, onAppearanceChange, disabled = false, children }: Props) {
    // 预览自动轮播：`override` 用于刚选中装饰时立刻插播它的动作。
    const [showcase, setShowcase] = useState<{ step: number; id: number; override?: AvatarAction }>({ step: 0, id: 0 });
    const reducedMotion = useReducedMotion();
    const definition = avatarDefinitions[appearance.shape];
    const current = showcase.override ? { kind: "action" as const, action: showcase.override }
        : showcaseStep(showcase.step, accessoryActionsOf([appearance.accessories.eyes, appearance.accessories.head]));
    const delay = showcaseDelay(current, showcase.id);
    useEffect(() => {
        if (reducedMotion) return;
        const timer = setTimeout(() => setShowcase(previous => ({ step: previous.step + 1, id: previous.id + 1 })), delay);
        return () => clearTimeout(timer);
    }, [showcase.id, delay, reducedMotion]);
    const update = (patch: (previous: ButlerAppearance) => ButlerAppearance) => onAppearanceChange(patch(appearance));

    const palette = (side: "primary" | "secondary", title: string, colors: string[][]) => <fieldset className="space-y-2.5">
        <legend className="mb-2.5 text-sm font-medium">{title}</legend>
        <div className="flex flex-wrap gap-2">
            {colors.map(([label, color]) => <button key={color} type="button" title={label}
                aria-label={`${title}：${label}`} aria-pressed={appearance.colors[side] === color}
                onClick={() => update(previous => ({ ...previous, colors: { ...previous.colors, [side]: color } }))}
                className="flex size-11 items-center justify-center rounded-full border border-black/15 outline-offset-4 focus-visible:outline-2 focus-visible:outline-ring aria-pressed:ring-2 aria-pressed:ring-foreground aria-pressed:ring-offset-2 aria-pressed:ring-offset-background"
                style={{ backgroundColor: color, color: side === "primary" ? "#111A2D" : "#FFFFFF" }}>
                {appearance.colors[side] === color && <Check className="size-4" />}
            </button>)}
        </div>
    </fieldset>;

    return <div className="min-h-0 flex-1 overflow-y-auto sm:grid sm:grid-cols-[220px_minmax(0,1fr)] sm:grid-rows-[minmax(0,1fr)] sm:gap-6 sm:overflow-hidden"
        data-aipp-slot="assistant-profile-editor">
        <div className="mb-6 flex flex-col items-center justify-center gap-4 rounded-xl bg-muted/40 px-4 py-6 sm:mb-0" data-aipp-slot="assistant-profile-preview">
            <ButlerAvatar id={0} chief size={200} appearance={{ ...appearance, motion: true }} animated
                state={current.kind === "state" ? current.state : "idle"}
                actionRequest={current.kind === "action" ? { action: current.action, sequence: showcase.id } : undefined} />
            <span className="max-w-full break-words text-center font-medium">{name.trim() || "助手"}</span>
        </div>
        {/* 右侧单独滚动；留出 1px 内边距，避免聚焦描边被裁掉。 */}
        <fieldset disabled={disabled} className="min-w-0 space-y-6 px-1 pb-1 sm:min-h-0 sm:overflow-y-auto sm:pr-3">
            {children && <div className="space-y-4">{children}</div>}
            <section aria-label="形象" className={`space-y-5 ${children ? "border-t border-border pt-6" : ""}`}>
                <div className="flex items-center justify-between gap-2">
                    <h3 className="text-sm font-semibold">形象</h3>
                    <Button type="button" variant="ghost" size="sm" className="h-7 text-muted-foreground"
                        onClick={() => update(previous => ({ ...previous, colors: defaultAppearance.colors }))}>恢复黑白配色</Button>
                </div>
                <fieldset className="space-y-2.5">
                    <legend className="mb-2.5 text-sm font-medium">形状</legend>
                    <div className="flex flex-wrap gap-2">
                        {Object.values(avatarDefinitions).map(shape => <button key={shape.id} type="button" aria-label={shape.label} title={shape.label}
                            aria-pressed={appearance.shape === shape.id}
                            onClick={() => update(previous => ({ ...previous, shape: shape.id }))}
                            className="flex items-center justify-center rounded-lg border p-2 aria-pressed:border-primary aria-pressed:bg-accent/40 focus-visible:outline-2 focus-visible:outline-ring">
                            <ButlerAvatar id={0} chief size={40} appearance={{ ...appearance, shape: shape.id, accessories: { eyes: null, head: null } }} />
                        </button>)}
                    </div>
                </fieldset>
                {palette("primary", `${definition.colorLabels.primary} · 亮色`, lightColors)}
                {palette("secondary", `${definition.colorLabels.secondary} · 深色`, darkColors)}
                {accessorySlots.map(([slot, label]) => <fieldset key={slot} className="space-y-2.5">
                    <legend className="mb-2.5 text-sm font-medium">{label}</legend>
                    <div className="flex flex-wrap gap-2">
                        {[null, ...accessoriesOf(slot)].map(value => <button key={value ?? "none"} type="button"
                            aria-label={`${label}：${value ? accessoryDefinitions[value].label : "无"}`} title={value ? accessoryDefinitions[value].label : "无"}
                            aria-pressed={appearance.accessories[slot] === value}
                            onClick={() => {
                                update(previous => ({ ...previous, accessories: { ...previous.accessories, [slot]: value } }));
                                if (value) setShowcase(previous => ({ ...previous, id: previous.id + 1, override: accessoryDefinitions[value].action }));
                            }}
                            className="flex size-11 items-center justify-center rounded-lg border aria-pressed:border-primary aria-pressed:bg-accent/40 focus-visible:outline-2 focus-visible:outline-ring">
                            {value ? <AccessoryIcon id={value} appearance={appearance} /> : <X className="size-4 text-muted-foreground" />}
                        </button>)}
                    </div>
                </fieldset>)}
            </section>
        </fieldset>
    </div>;
}
