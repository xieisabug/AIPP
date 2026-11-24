import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { Check, Plus, Puzzle, Sparkles, X } from "lucide-react";
import { useReducedMotion } from "motion/react";
import { toast } from "sonner";
import {
    avatarDefinitions,
    type AccessoryEntry,
    type AvatarAction,
    type AvatarDefinition,
    type BuiltinAccessoryId,
    type FaceAccessory,
    type HeadAccessory,
} from "@/components/avatar/avatarDefinition";
import {
    extractPluginCodeFromAssetId,
    useAvatarRegistrySnapshot,
} from "@/components/avatar/avatarRegistry";
import { DeclarativeArtwork } from "@/components/avatar/DeclarativeArtwork";
import { coreColorOf } from "@/components/avatar/AnimatedAvatar";
import {
    accessoryPreviewViewBox,
    BaseballCapArtwork,
    CrownArtwork,
    HeadphonesArtwork,
    BlushArtwork,
    BowBellArtwork,
    GlassesArtwork,
    HatArtwork,
    FlowerArtwork,
    SunglassesArtwork,
    ThickBrowsArtwork,
} from "@/components/avatar/AvatarAccessories";
import {
    MustacheArtwork,
    BandageArtwork,
    EyepatchStrap,
    EyepatchArtwork,
    RotorArtwork,
    SproutArtwork,
    NightcapArtwork,
} from "@/components/avatar/PlayfulAccessories";
import { Button } from "@/components/ui/button";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { navigateMobileConfig } from "@/mobile/navigation";
import ButlerAvatar from "./ButlerAvatar";
import { defaultAppearance, type ButlerAppearance } from "./butlerAppearance";
import { accessoryActionsOf, showcaseDelay, showcaseStep } from "./avatarShowcase";

const lightColors = [
    ["纯白", "#FFFFFF"], ["奶油黄", "#FFF0B5"], ["蜜桃", "#FFD6C2"],
    ["樱花粉", "#FAD4E4"], ["浅紫", "#E4DAFF"], ["浅蓝", "#CDE9FF"],
    ["薄荷绿", "#CEF3DF"], ["青柠", "#E5F4B8"],
];
const terracotta = "#D87855";
const darkColors = [
    ["纯黑", "#000000"], ["石墨", "#343B48"], ["海军蓝", "#193B68"],
    ["靛蓝", "#39316D"], ["深紫", "#613C72"], ["酒红", "#793D50"],
    ["墨绿", "#205448"], ["深棕", "#624735"], ["陶橘", terracotta], ["鲸蓝", "#4D6BFE"],
];

const accessorySlots = [["eyes", "脸部"], ["head", "头部"]] as const;

/** 勾选标记的颜色：按色块的感知亮度在深色与白色间取，任意自定义颜色上都看得清。 */
function markColor(hex: string) {
    const [r, g, b] = [1, 3, 5].map(index => parseInt(hex.slice(index, index + 2), 16));
    return 0.299 * r + 0.587 * g + 0.114 * b > 160 ? "#111A2D" : "#FFFFFF";
}

function AccessoryIcon({ entry, appearance }: { entry: AccessoryEntry; appearance: ButlerAppearance }) {
    if (entry.plugin) {
        const palette = {
            primary: appearance.colors.primary,
            secondary: appearance.colors.secondary,
            core: coreColorOf(appearance),
        };
        return <svg aria-hidden="true" viewBox={entry.plugin.previewViewBox} className="size-8">
            <DeclarativeArtwork layers={entry.plugin.layers} palette={palette} />
        </svg>;
    }
    const id = entry.id as BuiltinAccessoryId;
    const artwork = {
        mustache: <MustacheArtwork />, bandage: <BandageArtwork />, eyepatch: <><EyepatchStrap /><EyepatchArtwork /></>,
        rotor: <RotorArtwork />, sprout: <SproutArtwork />, nightcap: <NightcapArtwork />,
        crown: <CrownArtwork />, headphones: <HeadphonesArtwork />,
        "thick-brows": <ThickBrowsArtwork />,
        "round-glasses": <GlassesArtwork />, sunglasses: <SunglassesArtwork />, blush: <BlushArtwork />, beret: <HatArtwork />, "baseball-cap": <BaseballCapArtwork />,
        "baseball-cap-left": <BaseballCapArtwork reversed />,
        "bow-bell": <BowBellArtwork color={appearance.colors.primary} outline={appearance.colors.secondary} />, flower: <FlowerArtwork />,
    }[id];
    return <svg aria-hidden="true" viewBox={accessoryPreviewViewBox[id]} className="size-8">{artwork}</svg>;
}

function groupByPlugin<T extends { pluginCode?: string; pluginName?: string }>(items: T[]) {
    const groups: { code: string; name: string; items: T[] }[] = [];
    const byCode = new Map<string, { code: string; name: string; items: T[] }>();
    for (const item of items) {
        if (!item.pluginCode) continue;
        let group = byCode.get(item.pluginCode);
        if (!group) {
            group = { code: item.pluginCode, name: item.pluginName || item.pluginCode, items: [] };
            byCode.set(item.pluginCode, group);
            groups.push(group);
        }
        group.items.push(item);
    }
    return groups;
}

async function openPluginCenter(mobile: boolean) {
    const navigation = { menu: "plugins-config" };
    if (mobile) {
        navigateMobileConfig(navigation.menu);
        return;
    }
    await emit("config-navigate-to", navigation);
    await invoke("open_config_window");
    // 隐藏的设置窗口有时在显示后才完成事件处理，补发一次确保切到插件页。
    await emit("config-navigate-to", navigation);
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
    const registry = useAvatarRegistrySnapshot();
    const isMobilePlatform = useIsMobilePlatform();
    // 预览自动轮播：`override` 用于刚选中装饰时立刻插播它的动作。
    const [showcase, setShowcase] = useState<{ step: number; id: number; override?: AvatarAction }>({ step: 0, id: 0 });
    const reducedMotion = useReducedMotion();
    const matchedDefinition = registry.avatarMap.get(appearance.shape);
    const definition: AvatarDefinition = matchedDefinition ?? avatarDefinitions.pebble;
    const missingPlugins = Array.from(new Set([
        !matchedDefinition ? extractPluginCodeFromAssetId(appearance.shape) : null,
        appearance.accessories.eyes && !registry.accessoryMap.has(appearance.accessories.eyes)
            ? extractPluginCodeFromAssetId(appearance.accessories.eyes)
            : null,
        appearance.accessories.head && !registry.accessoryMap.has(appearance.accessories.head)
            ? extractPluginCodeFromAssetId(appearance.accessories.head)
            : null,
    ].filter((code): code is string => !!code)));

    const current = showcase.override ? { kind: "action" as const, action: showcase.override }
        : showcaseStep(showcase.step, accessoryActionsOf([appearance.accessories.eyes, appearance.accessories.head]));
    const delay = showcaseDelay(current, showcase.id, appearance.shape);
    useEffect(() => {
        if (reducedMotion) return;
        const timer = setTimeout(() => setShowcase(previous => ({ step: previous.step + 1, id: previous.id + 1 })), delay);
        return () => clearTimeout(timer);
    }, [showcase.id, delay, reducedMotion]);
    const update = (patch: (previous: ButlerAppearance) => ButlerAppearance) => onAppearanceChange(patch(appearance));

    const builtinShapes = registry.avatars.filter(item => !item.pluginCode);
    const pluginShapeGroups = groupByPlugin(registry.pluginAvatars);

    const handleOpenPluginCenter = () => {
        openPluginCenter(isMobilePlatform).catch(error => {
            toast.error(`打开插件中心失败：${error instanceof Error ? error.message : String(error)}`);
        });
    };

    const palette = (side: "primary" | "secondary", title: string, colors: string[][]) => {
        const current = appearance.colors[side];
        const setColor = (color: string) => update(previous => ({ ...previous, colors: { ...previous.colors, [side]: color } }));
        // 预设之外的颜色都算自定义；系统取色器返回小写，统一转大写后与预设比较。
        const custom = !colors.some(([, color]) => color === current.toUpperCase());
        return <fieldset className="space-y-2.5">
            <legend className="mb-2.5 text-sm font-medium">{title}</legend>
            <div className="flex flex-wrap gap-2">
                {colors.map(([label, color]) => <button key={color} type="button" title={label}
                    aria-label={`${title}：${label}`} aria-pressed={current.toUpperCase() === color}
                    onClick={() => setColor(color)}
                    className="flex size-11 items-center justify-center rounded-full border border-black/15 outline-offset-4 focus-visible:outline-2 focus-visible:outline-ring aria-pressed:ring-2 aria-pressed:ring-foreground aria-pressed:ring-offset-2 aria-pressed:ring-offset-background"
                    style={{ backgroundColor: color, color: markColor(color) }}>
                    {current.toUpperCase() === color && <Check className="size-4" />}
                </button>)}
                <label title="自定义颜色" data-custom={custom || undefined}
                    className="relative flex size-11 cursor-pointer items-center justify-center rounded-full border border-dashed border-muted-foreground/60 text-muted-foreground outline-offset-4 has-focus-visible:outline-2 has-focus-visible:outline-ring data-custom:border-solid data-custom:border-black/15 data-custom:ring-2 data-custom:ring-foreground data-custom:ring-offset-2 data-custom:ring-offset-background"
                    style={custom ? { backgroundColor: current, color: markColor(current) } : undefined}>
                    {custom ? <Check className="size-4" /> : <Plus className="size-4" />}
                    {/* 透明的原生取色器盖满色块：点击即打开系统取色面板，面板也锚定在色块位置。 */}
                    <input type="color" aria-label={`${title}：自定义颜色`} value={current.toLowerCase()}
                        onChange={event => setColor(event.target.value.toUpperCase())}
                        className="absolute inset-0 size-full cursor-pointer rounded-full opacity-0" />
                </label>
            </div>
        </fieldset>;
    };

    const renderShapeButton = (shape: AvatarDefinition) => (
        <button key={shape.id} type="button" aria-label={shape.label} title={shape.label}
            aria-pressed={appearance.shape === shape.id}
            onClick={() => update(previous => ({ ...previous, shape: shape.id }))}
            className="flex items-center justify-center rounded-lg border p-2 aria-pressed:border-primary aria-pressed:bg-accent/40 focus-visible:outline-2 focus-visible:outline-ring">
            <ButlerAvatar id={0} chief size={40} appearance={{ ...appearance, shape: shape.id, accessories: { eyes: null, head: null } }} />
        </button>
    );

    const hasRecommendedColors = !!(definition.defaultColors?.primary || definition.defaultColors?.secondary);

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
                <div className="flex flex-wrap items-center justify-between gap-2">
                    <h3 className="text-sm font-semibold">形象</h3>
                    <div className="flex flex-wrap items-center gap-1">
                        {hasRecommendedColors && (
                            <Button type="button" variant="ghost" size="sm" className="h-7 gap-1 text-muted-foreground"
                                onClick={() => update(previous => ({
                                    ...previous,
                                    colors: {
                                        primary: definition.defaultColors?.primary ?? previous.colors.primary,
                                        secondary: definition.defaultColors?.secondary ?? previous.colors.secondary,
                                    },
                                }))}>
                                <Sparkles className="size-3.5" />
                                推荐配色
                            </Button>
                        )}
                        <Button type="button" variant="ghost" size="sm" className="h-7 text-muted-foreground"
                            onClick={() => update(previous => ({ ...previous, colors: definition.colorLabels.primary ? defaultAppearance.colors : { ...previous.colors, secondary: defaultAppearance.colors.secondary } }))}>
                            {definition.colorLabels.primary ? "恢复黑白配色" : "恢复黑色"}
                        </Button>
                        <Button type="button" variant="ghost" size="sm" className="h-7 gap-1 text-muted-foreground"
                            onClick={handleOpenPluginCenter}>
                            <Puzzle className="size-3.5" />
                            更多形象
                        </Button>
                    </div>
                </div>
                {missingPlugins.length > 0 && (
                    <div role="status" className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-muted-foreground">
                        当前配置的部分形象或配饰来自未安装或已禁用的插件（{missingPlugins.join("、")}），已临时以默认外观显示。重新启用对应插件即可自动恢复，或直接切换其他选项。
                    </div>
                )}
                <fieldset className="space-y-2.5">
                    <legend className="mb-2.5 text-sm font-medium">形状</legend>
                    <div className="flex flex-wrap gap-2">
                        {builtinShapes.map(renderShapeButton)}
                    </div>
                    {pluginShapeGroups.map(group => (
                        <div key={group.code} className="space-y-1.5 pt-1">
                            <div className="text-xs text-muted-foreground">{group.name}</div>
                            <div className="flex flex-wrap gap-2">
                                {group.items.map(renderShapeButton)}
                            </div>
                        </div>
                    ))}
                </fieldset>
                {definition.colorLabels.primary && palette("primary", definition.colorLabels.primary, lightColors)}
                {palette("secondary", definition.colorLabels.secondary, darkColors)}
                {accessorySlots.map(([slot, label]) => {
                    const builtinItems = registry.accessories.filter(item => item.slot === slot && !item.plugin);
                    const pluginItems = registry.accessories.filter(item => item.slot === slot && !!item.plugin);
                    const pluginGroups = groupByPlugin(
                        pluginItems.map(item => ({
                            entry: item,
                            pluginCode: item.plugin?.pluginCode,
                            pluginName: item.plugin?.pluginName,
                        })),
                    );
                    const selectAccessory = (entry: AccessoryEntry | null) => {
                        const nextVal = entry ? entry.id : null;
                        update(previous => ({
                            ...previous,
                            accessories: slot === "eyes"
                                ? { ...previous.accessories, eyes: nextVal as FaceAccessory | null }
                                : { ...previous.accessories, head: nextVal as HeadAccessory | null },
                        }));
                        const previewAction = entry?.action;
                        if (previewAction) setShowcase(previous => ({ ...previous, id: previous.id + 1, override: previewAction }));
                    };
                    const renderAccessoryButton = (entry: AccessoryEntry) => (
                        <button key={entry.id} type="button"
                            aria-label={`${label}：${entry.label}`} title={entry.label}
                            aria-pressed={appearance.accessories[slot] === entry.id}
                            onClick={() => selectAccessory(entry)}
                            className="flex size-11 items-center justify-center rounded-lg border aria-pressed:border-primary aria-pressed:bg-accent/40 focus-visible:outline-2 focus-visible:outline-ring">
                            <AccessoryIcon entry={entry} appearance={appearance} />
                        </button>
                    );

                    return <fieldset key={slot} className="space-y-2.5">
                        <legend className="mb-2.5 text-sm font-medium">{label}</legend>
                        <div className="flex flex-wrap gap-2">
                            <button key="none" type="button"
                                aria-label={`${label}：无`} title="无"
                                aria-pressed={appearance.accessories[slot] === null}
                                onClick={() => selectAccessory(null)}
                                className="aipp-icon-button flex size-11 items-center justify-center rounded-lg border aria-pressed:border-primary aria-pressed:bg-accent/40 focus-visible:outline-2 focus-visible:outline-ring">
                                <X className="size-4 text-muted-foreground" />
                            </button>
                            {builtinItems.map(renderAccessoryButton)}
                        </div>
                        {pluginGroups.map(group => (
                            <div key={group.code} className="space-y-1.5 pt-1">
                                <div className="text-xs text-muted-foreground">{group.name}</div>
                                <div className="flex flex-wrap gap-2">
                                    {group.items.map(item => renderAccessoryButton(item.entry))}
                                </div>
                            </div>
                        ))}
                    </fieldset>;
                })}
            </section>
        </fieldset>
    </div>;
}
