import { useState } from "react";
import { Check, Loader2 } from "lucide-react";
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
    const palette = (side: "left" | "right", title: string, colors: string[][]) => <fieldset className="space-y-3">
        <legend className="text-sm font-medium">{title}</legend>
        <div className="flex flex-wrap gap-2">
            {colors.map(([label, color]) => <button key={color} type="button" title={label}
                aria-label={`${title}：${label}`} aria-pressed={appearance[side] === color}
                onClick={() => setAppearance(previous => ({ ...previous, [side]: color }))}
                className="flex size-8 items-center justify-center rounded-full border border-black/15 outline-offset-4 focus-visible:outline-2 focus-visible:outline-ring aria-pressed:ring-2 aria-pressed:ring-foreground aria-pressed:ring-offset-2 aria-pressed:ring-offset-background"
                style={{ backgroundColor: color, color: side === "left" ? "#111A2D" : "#FFFFFF" }}>
                {appearance[side] === color && <Check className="size-4" />}
            </button>)}
        </div>
    </fieldset>;

    return <Dialog open onOpenChange={open => { if (!open && !saving) onClose(); }}>
        <DialogContent className="sm:max-w-[660px] max-h-[90vh] overflow-y-auto" showCloseButton={!saving}>
            <DialogHeader>
                <DialogTitle>名字与形象</DialogTitle>
                <DialogDescription>为助手选一个名字，搭配属于它的颜色。</DialogDescription>
            </DialogHeader>
            <form onSubmit={event => { event.preventDefault(); void save(); }} className="space-y-6">
                <fieldset disabled={saving} className="grid gap-6 sm:grid-cols-[180px_1fr]">
                    <div className="flex flex-col items-center justify-center gap-4 rounded-xl bg-muted/40 px-4 py-6">
                        <ButlerAvatar id={0} chief size={128} appearance={appearance} />
                        <span className="max-w-full break-words text-center font-medium">{name.trim() || "助手"}</span>
                    </div>
                    <div className="min-w-0 space-y-5">
                        <div className="space-y-2">
                            <label htmlFor="butler-profile-name" className="text-sm font-medium">助手名称</label>
                            <Input id="butler-profile-name" value={name} onChange={event => setName(event.target.value)} maxLength={80} autoFocus />
                        </div>
                        <fieldset className="space-y-2">
                            <legend className="text-sm font-medium">形状</legend>
                            <button type="button" aria-label="AIPP Logo" aria-pressed="true" className="flex items-center justify-center rounded-lg border border-primary bg-accent/40 p-2">
                                <ButlerAvatar id={0} chief size={32} appearance={appearance} />
                            </button>
                        </fieldset>
                        {palette("left", "左侧 · 亮色", lightColors)}
                        {palette("right", "右侧 · 深色", darkColors)}
                    </div>
                </fieldset>
                {error && <p role="alert" className="text-sm text-destructive break-words">{error}</p>}
                <DialogFooter>
                    <Button type="button" variant="ghost" className="sm:mr-auto" disabled={saving} onClick={() => setAppearance(defaultAppearance)}>恢复黑白配色</Button>
                    <Button type="button" variant="outline" disabled={saving} onClick={onClose}>取消</Button>
                    <Button type="submit" disabled={saving || !name.trim()}>{saving && <Loader2 className="size-4 animate-spin" />}{saving ? "正在保存…" : "保存"}</Button>
                </DialogFooter>
            </form>
        </DialogContent>
    </Dialog>;
}
