import { useEffect, useState } from "react";
import { Check, Loader2, X } from "lucide-react";
import { useReducedMotion } from "motion/react";
import { accessoriesOf, accessoryDefinitions, avatarDefinitions, type AccessoryId, type AvatarAction } from "@/components/avatar/avatarDefinition";
import { accessoryPreviewViewBox, BlushArtwork, BowBellArtwork, GlassesArtwork, HatArtwork, FlowerArtwork, SunglassesArtwork } from "@/components/avatar/AvatarAccessories";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
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

function AccessoryIcon({ id, appearance }: { id: AccessoryId; appearance: ButlerAppearance }) {
    const artwork = {
        "round-glasses": <GlassesArtwork />, sunglasses: <SunglassesArtwork />, blush: <BlushArtwork />, beret: <HatArtwork />,
        "bow-bell": <BowBellArtwork color={appearance.colors.primary} outline={appearance.colors.secondary} />, flower: <FlowerArtwork />,
    }[id];
    return <svg aria-hidden="true" viewBox={accessoryPreviewViewBox[id]} className="size-8">{artwork}</svg>;
}

interface Props {
    name: string;
    appearance?: ButlerAppearance;
    description?: string;
    onClose: () => void;
    onSave: (name: string, appearance: ButlerAppearance, description?: string) => Promise<void>;
}

export default function ButlerProfileDialog({ name: initialName, appearance: initialAppearance, description: initialDescription, onClose, onSave }: Props) {
    const [name, setName] = useState(initialName);
    const [description, setDescription] = useState(initialDescription ?? "");
    const [appearance, setAppearance] = useState(initialAppearance ?? defaultAppearance);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState("");
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
    const save = async () => {
        if (!name.trim() || saving) return;
        setSaving(true);
        setError("");
        try {
            // 编辑器不再提供动画开关，保存时统一开启（兼容旧配置里关掉的情况）。
            if (initialDescription === undefined) {
                await onSave(name.trim(), { ...appearance, motion: true });
            } else {
                await onSave(name.trim(), { ...appearance, motion: true }, description);
            }
            onClose();
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setSaving(false);
        }
    };
    const palette = (side: "primary" | "secondary", title: string, colors: string[][]) => <fieldset className="space-y-3">
        <legend className="text-sm font-medium">{title}</legend>
        <div className="flex flex-wrap gap-2">
            {colors.map(([label, color]) => <button key={color} type="button" title={label}
                aria-label={`${title}：${label}`} aria-pressed={appearance.colors[side] === color}
                onClick={() => setAppearance(previous => ({ ...previous, colors: { ...previous.colors, [side]: color } }))}
                className="flex size-11 items-center justify-center rounded-full border border-black/15 outline-offset-4 focus-visible:outline-2 focus-visible:outline-ring aria-pressed:ring-2 aria-pressed:ring-foreground aria-pressed:ring-offset-2 aria-pressed:ring-offset-background"
                style={{ backgroundColor: color, color: side === "primary" ? "#111A2D" : "#FFFFFF" }}>
                {appearance.colors[side] === color && <Check className="size-4" />}
            </button>)}
        </div>
    </fieldset>;

    return <Dialog open onOpenChange={open => { if (!open && !saving) onClose(); }}>
        <DialogContent className="sm:max-w-[720px] max-h-[90vh] overflow-y-auto" showCloseButton={!saving}>
            <DialogHeader>
                <DialogTitle>{initialDescription === undefined ? "名字与形象" : "名字、描述与形象"}</DialogTitle>
                <DialogDescription>为助手选一个名字，搭配属于它的颜色。</DialogDescription>
            </DialogHeader>
            <form onSubmit={event => { event.preventDefault(); void save(); }} className="space-y-6">
                <fieldset disabled={saving} className="grid gap-6 sm:grid-cols-[220px_1fr]">
                    <div className="flex flex-col items-center gap-4 rounded-xl bg-muted/40 px-4 py-6">
                        <ButlerAvatar id={0} chief size={200} appearance={{ ...appearance, motion: true }} animated
                            state={current.kind === "state" ? current.state : "idle"}
                            actionRequest={current.kind === "action" ? { action: current.action, sequence: showcase.id } : undefined} />
                        <span className="max-w-full break-words text-center font-medium">{name.trim() || "助手"}</span>
                    </div>
                    <div className="min-w-0 space-y-5">
                        <div className="space-y-2">
                            <label htmlFor="butler-profile-name" className="text-sm font-medium">助手名称</label>
                            <Input id="butler-profile-name" value={name} onChange={event => setName(event.target.value)} maxLength={80} autoFocus />
                        </div>
                        {initialDescription !== undefined && <div className="space-y-2">
                            <label htmlFor="assistant-profile-description" className="text-sm font-medium">描述（选填）</label>
                            <Input id="assistant-profile-description" value={description} onChange={event => setDescription(event.target.value)} />
                        </div>}
                        <fieldset className="space-y-2">
                            <legend className="text-sm font-medium">形状</legend>
                            <div className="flex flex-wrap gap-2">
                                {Object.values(avatarDefinitions).map(shape => <button key={shape.id} type="button" aria-label={shape.label} title={shape.label}
                                    aria-pressed={appearance.shape === shape.id}
                                    onClick={() => setAppearance(previous => ({ ...previous, shape: shape.id }))}
                                    className="flex items-center justify-center rounded-lg border p-2 aria-pressed:border-primary aria-pressed:bg-accent/40 focus-visible:outline-2 focus-visible:outline-ring">
                                    <ButlerAvatar id={0} chief size={40} appearance={{ ...appearance, shape: shape.id, accessories: { eyes: null, head: null } }} />
                                </button>)}
                            </div>
                        </fieldset>
                        {palette("primary", `${definition.colorLabels.primary} · 亮色`, lightColors)}
                        {palette("secondary", `${definition.colorLabels.secondary} · 深色`, darkColors)}
                        {accessorySlots.map(([slot, label]) => <fieldset key={slot} className="space-y-2">
                            <legend className="text-sm font-medium">{label}</legend>
                            <div className="flex flex-wrap gap-2">
                                {[null, ...accessoriesOf(slot)].map(value => <button key={value ?? "none"} type="button"
                                    aria-label={`${label}：${value ? accessoryDefinitions[value].label : "无"}`} title={value ? accessoryDefinitions[value].label : "无"}
                                    aria-pressed={appearance.accessories[slot] === value}
                                    onClick={() => {
                                        setAppearance(previous => ({ ...previous, accessories: { ...previous.accessories, [slot]: value } }));
                                        if (value) setShowcase(previous => ({ ...previous, id: previous.id + 1, override: accessoryDefinitions[value].action }));
                                    }}
                                    className="flex size-11 items-center justify-center rounded-lg border aria-pressed:border-primary aria-pressed:bg-accent/40 focus-visible:outline-2 focus-visible:outline-ring">
                                    {value ? <AccessoryIcon id={value} appearance={appearance} /> : <X className="size-4 text-muted-foreground" />}
                                </button>)}
                            </div>
                        </fieldset>)}
                    </div>
                </fieldset>
                {error && <p role="alert" className="text-sm text-destructive break-words">{error}</p>}
                <DialogFooter>
                    <Button type="button" variant="ghost" className="sm:mr-auto" disabled={saving} onClick={() => setAppearance(previous => ({ ...previous, colors: defaultAppearance.colors }))}>恢复黑白配色</Button>
                    <Button type="button" variant="outline" disabled={saving} onClick={onClose}>取消</Button>
                    <Button type="submit" disabled={saving || !name.trim()}>{saving && <Loader2 className="size-4 animate-spin" />}{saving ? "正在保存…" : "保存"}</Button>
                </DialogFooter>
            </form>
        </DialogContent>
    </Dialog>;
}
