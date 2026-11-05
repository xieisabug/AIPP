import { useState } from "react";
import { Check, Loader2, X } from "lucide-react";
import { useReducedMotion } from "motion/react";
import { avatarDefinitions, avatarStateLabels, type AvatarAction, type AvatarState } from "@/components/avatar/avatarDefinition";
import { GlassesArtwork, HatArtwork } from "@/components/avatar/AvatarAccessories";
import type { AvatarActionRequest } from "@/components/avatar/useAvatarMotion";
import { avatarRecipes } from "@/components/avatar/avatarRecipes";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import ButlerAvatar from "./ButlerAvatar";
import { defaultAppearance, type ButlerAppearance } from "./butlerAppearance";

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

interface Props {
    name: string;
    appearance?: ButlerAppearance;
    onClose: () => void;
    onSave: (name: string, appearance: ButlerAppearance) => Promise<void>;
}

export default function ButlerProfileDialog({ name: initialName, appearance: initialAppearance, onClose, onSave }: Props) {
    const [name, setName] = useState(initialName);
    const [appearance, setAppearance] = useState(initialAppearance ?? defaultAppearance);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState("");
    const [previewState, setPreviewState] = useState<AvatarState>("idle");
    const [actionRequest, setActionRequest] = useState<AvatarActionRequest>();
    const reducedMotion = useReducedMotion();
    const definition = avatarDefinitions[appearance.shape];
    const play = (action: AvatarAction) => setActionRequest(previous => ({ action, sequence: (previous?.sequence ?? 0) + 1 }));
    const save = async () => {
        if (!name.trim() || saving) return;
        setSaving(true);
        setError("");
        try {
            await onSave(name.trim(), appearance);
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
                className="flex size-8 items-center justify-center rounded-full border border-black/15 outline-offset-4 focus-visible:outline-2 focus-visible:outline-ring aria-pressed:ring-2 aria-pressed:ring-foreground aria-pressed:ring-offset-2 aria-pressed:ring-offset-background"
                style={{ backgroundColor: color, color: side === "primary" ? "#111A2D" : "#FFFFFF" }}>
                {appearance.colors[side] === color && <Check className="size-4" />}
            </button>)}
        </div>
    </fieldset>;

    return <Dialog open onOpenChange={open => { if (!open && !saving) onClose(); }}>
        <DialogContent className="sm:max-w-[720px] max-h-[90vh] overflow-y-auto" showCloseButton={!saving}>
            <DialogHeader>
                <DialogTitle>名字与形象</DialogTitle>
                <DialogDescription>为助手选一个名字，搭配属于它的颜色。</DialogDescription>
            </DialogHeader>
            <form onSubmit={event => { event.preventDefault(); void save(); }} className="space-y-6">
                <fieldset disabled={saving} className="grid gap-6 sm:grid-cols-[220px_1fr]">
                    <div className="flex flex-col items-center gap-4 rounded-xl bg-muted/40 px-4 py-6">
                        <ButlerAvatar id={0} chief size={200} appearance={appearance} animated state={previewState} actionRequest={actionRequest} />
                        <span className="max-w-full break-words text-center font-medium">{name.trim() || "助手"}</span>
                        <label className="mt-auto flex w-full items-center justify-between gap-2 text-xs text-muted-foreground">
                            动作预览
                            <select aria-label="动作预览" value={previewState} onChange={event => { setPreviewState(event.target.value as AvatarState); setActionRequest(undefined); }}
                                className="rounded-md border bg-background px-2 py-1 text-foreground">
                                {["日常", "情绪", "图形", "互动", "展示"].map(group => <optgroup key={group} label={group}>
                                    {Object.entries(avatarRecipes).filter(([,recipe]) => recipe.group === group).map(([value, recipe]) => <option key={value} value={value}>{recipe.label}</option>)}
                                </optgroup>)}
                            </select>
                        </label>
                        <div className="flex flex-wrap justify-center gap-1" aria-label="常用状态">
                            {(["idle", "listening", "thinking", "working", "waiting", "celebrate"] as const).map(value => <Button key={value} type="button" size="sm"
                                variant={previewState === value ? "secondary" : "ghost"} aria-pressed={previewState === value}
                                onClick={() => { setPreviewState(value); setActionRequest(undefined); }}>{avatarStateLabels[value]}</Button>)}
                        </div>
                        <div className="flex flex-wrap justify-center gap-1">
                            {([ ["blink", "眨眼"], ["smile", "微笑"], ["glasses", "摘戴眼镜"], ["hat", "抬帽"] ] as const).map(([action, label]) =>
                                <Button key={action} type="button" size="sm" variant="ghost" onClick={() => play(action)}
                                    disabled={!appearance.motion || !!reducedMotion || (action === "glasses" && !appearance.accessories.eyes) || (action === "hat" && !appearance.accessories.head)}>{label}</Button>)}
                        </div>
                    </div>
                    <div className="min-w-0 space-y-5">
                        <div className="space-y-2">
                            <label htmlFor="butler-profile-name" className="text-sm font-medium">助手名称</label>
                            <Input id="butler-profile-name" value={name} onChange={event => setName(event.target.value)} maxLength={80} autoFocus />
                        </div>
                        <fieldset className="space-y-2">
                            <legend className="text-sm font-medium">形状</legend>
                            <div className="flex gap-2">
                                {Object.values(avatarDefinitions).map(shape => <button key={shape.id} type="button" aria-label={shape.label} title={shape.label}
                                    aria-pressed={appearance.shape === shape.id}
                                    onClick={() => { setActionRequest(undefined); setAppearance(previous => ({ ...previous, shape: shape.id })); }}
                                    className="flex items-center justify-center rounded-lg border p-2 aria-pressed:border-primary aria-pressed:bg-accent/40 focus-visible:outline-2 focus-visible:outline-ring">
                                    <ButlerAvatar id={0} chief size={40} appearance={{ ...appearance, shape: shape.id, accessories: { eyes: null, head: null } }} />
                                </button>)}
                            </div>
                        </fieldset>
                        {palette("primary", `${definition.colorLabels.primary} · 亮色`, lightColors)}
                        {palette("secondary", `${definition.colorLabels.secondary} · 深色`, darkColors)}
                        <div className="flex gap-6">
                            {([ ["eyes", "眼镜", "round-glasses"], ["head", "帽子", "beret"] ] as const).map(([slot, label, accessory]) => <fieldset key={slot} className="space-y-2">
                                <legend className="text-sm font-medium">{label}</legend>
                                <div className="flex gap-2">
                                    {[null, accessory].map(value => <button key={value ?? "none"} type="button" aria-label={value ? `佩戴${label}` : `不戴${label}`}
                                        aria-pressed={appearance.accessories[slot] === value}
                                        onClick={() => { setActionRequest(undefined); setAppearance(previous => ({ ...previous, accessories: { ...previous.accessories, [slot]: value } })); }}
                                        className="flex size-11 items-center justify-center rounded-lg border aria-pressed:border-primary aria-pressed:bg-accent/40 focus-visible:outline-2 focus-visible:outline-ring">
                                        {value ? <svg aria-hidden="true" viewBox={slot === "eyes" ? "-115 -65 230 130" : "-140 -100 280 140"} className="size-8">{slot === "eyes" ? <GlassesArtwork /> : <HatArtwork />}</svg> : <X className="size-4 text-muted-foreground" />}
                                    </button>)}
                                </div>
                            </fieldset>)}
                        </div>
                        <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={appearance.motion}
                            onChange={event => setAppearance(previous => ({ ...previous, motion: event.target.checked }))} />播放动画</label>
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
