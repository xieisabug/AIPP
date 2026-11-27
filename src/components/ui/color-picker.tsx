import * as React from "react";

import { cn } from "@/utils/utils";
import { Input } from "@/components/ui/input";

interface Hsv {
    /** 0–360 */
    h: number;
    /** 0–1 */
    s: number;
    /** 0–1 */
    v: number;
}

const HEX_PATTERN = /^#?([0-9a-fA-F]{6})$/;

/** 把 `#RRGGBB`（`#` 可省略、大小写不限）规范成大写 `#RRGGBB`；不合法返回 null。 */
export function normalizeHex(text: string): string | null {
    const match = HEX_PATTERN.exec(text.trim());
    return match ? `#${match[1].toUpperCase()}` : null;
}

export function hexToHsv(hex: string): Hsv {
    const normalized = normalizeHex(hex) ?? "#000000";
    const [r, g, b] = [1, 3, 5].map((index) => parseInt(normalized.slice(index, index + 2), 16) / 255);
    const max = Math.max(r, g, b);
    const delta = max - Math.min(r, g, b);
    let h = 0;
    if (delta > 0) {
        if (max === r) h = ((g - b) / delta) % 6;
        else if (max === g) h = (b - r) / delta + 2;
        else h = (r - g) / delta + 4;
        h *= 60;
        if (h < 0) h += 360;
    }
    return { h, s: max === 0 ? 0 : delta / max, v: max };
}

export function hsvToHex({ h, s, v }: Hsv): string {
    const channel = (n: number) => {
        const k = (n + h / 60) % 6;
        const value = v - v * s * Math.max(0, Math.min(k, 4 - k, 1));
        return Math.round(value * 255).toString(16).padStart(2, "0");
    };
    return `#${channel(5)}${channel(3)}${channel(1)}`.toUpperCase();
}

const clamp = (value: number, min = 0, max = 1) => Math.min(max, Math.max(min, value));

/** 指针拖动：按下时捕获指针，移动时回调相对元素的 0–1 坐标。每次渲染重新生成，回调总是最新的。 */
function pointerDragHandlers(onMove: (x: number, y: number) => void) {
    const report = (event: React.PointerEvent<HTMLElement>) => {
        const rect = event.currentTarget.getBoundingClientRect();
        const x = rect.width ? clamp((event.clientX - rect.left) / rect.width) : 0;
        const y = rect.height ? clamp((event.clientY - rect.top) / rect.height) : 0;
        onMove(x, y);
    };
    return {
        onPointerDown: (event: React.PointerEvent<HTMLElement>) => {
            if (event.button !== 0) return;
            event.preventDefault();
            event.currentTarget.focus();
            event.currentTarget.setPointerCapture?.(event.pointerId);
            report(event);
        },
        onPointerMove: (event: React.PointerEvent<HTMLElement>) => {
            if (event.currentTarget.hasPointerCapture?.(event.pointerId)) report(event);
        },
        onPointerUp: (event: React.PointerEvent<HTMLElement>) => {
            event.currentTarget.releasePointerCapture?.(event.pointerId);
        },
    };
}

export interface ColorPickerProps {
    /** `#RRGGBB`。 */
    value: string;
    /** 回传大写 `#RRGGBB`。 */
    onChange: (value: string) => void;
    /** 无障碍名称前缀，各控件名称为「前缀 + 饱和度与亮度 / 色相 / 十六进制」。 */
    label?: string;
    className?: string;
}

/** 应用内取色器：饱和度/亮度面板 + 色相条 + 十六进制输入，代替会弹出系统取色面板的 `<input type="color">`。 */
function ColorPicker({ value, onChange, label = "颜色", className }: ColorPickerProps) {
    const normalizedValue = normalizeHex(value) ?? "#000000";
    // 内部保留 HSV：灰色或黑色时色相无法从十六进制还原，拖动时不能让色相跳回 0。
    const [hsv, setHsv] = React.useState<Hsv>(() => hexToHsv(normalizedValue));
    const [hexText, setHexText] = React.useState(normalizedValue);

    React.useEffect(() => {
        setHsv((previous) => (hsvToHex(previous) === normalizedValue ? previous : hexToHsv(normalizedValue)));
        setHexText((previous) => (normalizeHex(previous) === normalizedValue ? previous : normalizedValue));
    }, [normalizedValue]);

    const commit = (next: Hsv) => {
        setHsv(next);
        const hex = hsvToHex(next);
        setHexText(hex);
        if (hex !== normalizedValue) onChange(hex);
    };

    const areaDrag = pointerDragHandlers((x, y) => commit({ ...hsv, s: x, v: 1 - y }));
    const hueDrag = pointerDragHandlers((x) => commit({ ...hsv, h: x * 360 }));

    const onAreaKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
        const step = event.shiftKey ? 0.1 : 0.01;
        const delta = ({
            ArrowLeft: [-step, 0],
            ArrowRight: [step, 0],
            ArrowUp: [0, step],
            ArrowDown: [0, -step],
        } as Record<string, [number, number]>)[event.key];
        if (!delta) return;
        event.preventDefault();
        commit({ ...hsv, s: clamp(hsv.s + delta[0]), v: clamp(hsv.v + delta[1]) });
    };

    const onHueKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
        const step = event.shiftKey ? 10 : 1;
        const delta = ({ ArrowLeft: -step, ArrowDown: -step, ArrowRight: step, ArrowUp: step } as Record<string, number>)[event.key];
        if (delta === undefined) return;
        event.preventDefault();
        commit({ ...hsv, h: clamp(hsv.h + delta, 0, 360) });
    };

    const hexInvalid = normalizeHex(hexText) === null;
    const saturation = Math.round(hsv.s * 100);
    const brightness = Math.round(hsv.v * 100);
    const hue = Math.round(hsv.h);

    return (
        <div data-slot="color-picker" className={cn("flex w-52 flex-col gap-3", className)}>
            <div
                role="slider"
                tabIndex={0}
                aria-label={`${label}：饱和度与亮度`}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={saturation}
                aria-valuetext={`饱和度 ${saturation}%，亮度 ${brightness}%`}
                className="relative h-32 w-full cursor-crosshair touch-none rounded-md border border-border outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
                style={{
                    backgroundColor: `hsl(${hsv.h}, 100%, 50%)`,
                    backgroundImage: "linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, transparent)",
                }}
                onKeyDown={onAreaKeyDown}
                {...areaDrag}
            >
                <span
                    aria-hidden
                    className="pointer-events-none absolute size-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow-[0_0_0_1px_rgba(0,0,0,0.4)]"
                    style={{ left: `${hsv.s * 100}%`, top: `${(1 - hsv.v) * 100}%`, backgroundColor: normalizedValue }}
                />
            </div>
            <div
                role="slider"
                tabIndex={0}
                aria-label={`${label}：色相`}
                aria-valuemin={0}
                aria-valuemax={360}
                aria-valuenow={hue}
                aria-orientation="horizontal"
                className="relative h-3 w-full cursor-pointer touch-none rounded-full border border-border outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
                style={{
                    backgroundImage:
                        "linear-gradient(to right, #f00 0%, #ff0 17%, #0f0 33%, #0ff 50%, #00f 67%, #f0f 83%, #f00 100%)",
                }}
                onKeyDown={onHueKeyDown}
                {...hueDrag}
            >
                <span
                    aria-hidden
                    className="pointer-events-none absolute top-1/2 size-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow-[0_0_0_1px_rgba(0,0,0,0.4)]"
                    style={{ left: `${(hsv.h / 360) * 100}%`, backgroundColor: `hsl(${hsv.h}, 100%, 50%)` }}
                />
            </div>
            <div className="flex items-center gap-2">
                <span
                    aria-hidden
                    className="size-8 shrink-0 rounded-md border border-border"
                    style={{ backgroundColor: normalizedValue }}
                />
                <Input
                    aria-label={`${label}：十六进制`}
                    aria-invalid={hexInvalid || undefined}
                    value={hexText}
                    maxLength={7}
                    spellCheck={false}
                    className="h-8 font-mono text-sm uppercase"
                    onChange={(event) => {
                        const text = event.target.value;
                        setHexText(text);
                        const hex = normalizeHex(text);
                        if (!hex) return;
                        setHsv(hexToHsv(hex));
                        if (hex !== normalizedValue) onChange(hex);
                    }}
                    onBlur={() => setHexText(normalizedValue)}
                />
            </div>
        </div>
    );
}

export { ColorPicker };
