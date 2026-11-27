import React from "react";
import { cn } from "@/utils/utils";

export interface QuotaWindow {
    group?: string;
    label: string;
    remainingPercent: number | null;
    remainingAmount?: number;
    currency?: string;
    resetsAt: string | null;
}

export interface QuotaReading {
    windows: QuotaWindow[];
    checkedAt: number;
    expiresAt: number;
}

/**
 * Distinct high-contrast ring palette (Apple Activity Rings inspired, legible in light & dark mode).
 * Order puts emerald first so a single-ring chart reads positively, while 3-ring charts form the
 * classic emerald / cyan / rose trio.
 */
export const QUOTA_RING_COLORS = [
    "#10B981", // Emerald
    "#06B6D4", // Cyan
    "#F43F5E", // Rose
    "#F59E0B", // Amber
    "#8B5CF6", // Violet
] as const;

const CURRENCY_SYMBOLS: Record<string, string> = { CNY: "¥", USD: "$" };

export function formatQuotaPercent(percent: number): string {
    return `${percent.toLocaleString(undefined, { maximumFractionDigits: 2 })}%`;
}

export function formatQuotaAmount(amount: number, currency?: string): string {
    if (currency) {
        const symbol = CURRENCY_SYMBOLS[currency];
        const formatted = amount.toLocaleString(undefined, {
            minimumFractionDigits: 2,
            maximumFractionDigits: 2,
        });
        return symbol ? `${symbol}${formatted}` : `${formatted} ${currency}`;
    }
    return amount.toLocaleString(undefined, { maximumFractionDigits: 2 });
}

export function formatWindowRemaining(window: QuotaWindow): string {
    if (window.remainingPercent != null) {
        return `剩余 ${formatQuotaPercent(window.remainingPercent)}`;
    }
    if (window.remainingAmount != null) {
        return `剩余 ${formatQuotaAmount(window.remainingAmount, window.currency)}`;
    }
    return "未提供剩余额度";
}

export interface QuotaRingItem {
    windowIndex: number;
    label: string;
    percent: number;
    color: string;
}

export interface QuotaRingCluster {
    group?: string;
    rings: QuotaRingItem[];
}

export type QuotaDisplayModel =
    | {
        kind: "rings";
        clusters: QuotaRingCluster[];
        colorByIndex: Map<number, string>;
        hasDepleted: boolean;
    }
    | {
        kind: "amount";
        primaryWindow: QuotaWindow;
        formattedAmount: string;
        colorByIndex: Map<number, string>;
        hasDepleted: boolean;
    }
    | {
        kind: "empty";
        colorByIndex: Map<number, string>;
        hasDepleted: boolean;
    };

export function classifyQuotaDisplay(windows: QuotaWindow[]): QuotaDisplayModel {
    const colorByIndex = new Map<number, string>();
    const percentEntries = windows
        .map((window, index) => ({ window, index }))
        .filter((entry) => entry.window.remainingPercent != null);

    if (percentEntries.length > 0) {
        const distinctGroups = new Set(
            percentEntries
                .map((entry) => entry.window.group)
                .filter((group): group is string => Boolean(group && group.trim())),
        );

        const clusters: QuotaRingCluster[] = [];
        if (distinctGroups.size >= 2) {
            const byGroup = new Map<string | undefined, typeof percentEntries>();
            for (const entry of percentEntries) {
                const key = entry.window.group;
                const list = byGroup.get(key);
                if (list) list.push(entry);
                else byGroup.set(key, [entry]);
            }
            for (const [group, entries] of byGroup) {
                const rings = entries.map((entry, ringIdx) => {
                    const color = QUOTA_RING_COLORS[ringIdx % QUOTA_RING_COLORS.length];
                    colorByIndex.set(entry.index, color);
                    return {
                        windowIndex: entry.index,
                        label: entry.window.label,
                        percent: Math.max(0, Math.min(100, entry.window.remainingPercent ?? 0)),
                        color,
                    };
                });
                clusters.push({ group, rings });
            }
        } else {
            const rings = percentEntries.map((entry, ringIdx) => {
                const color = QUOTA_RING_COLORS[ringIdx % QUOTA_RING_COLORS.length];
                colorByIndex.set(entry.index, color);
                return {
                    windowIndex: entry.index,
                    label: entry.window.label,
                    percent: Math.max(0, Math.min(100, entry.window.remainingPercent ?? 0)),
                    color,
                };
            });
            clusters.push({ group: percentEntries[0].window.group, rings });
        }

        const hasDepleted = percentEntries.some((entry) => (entry.window.remainingPercent ?? 0) <= 0);
        return { kind: "rings", clusters, colorByIndex, hasDepleted };
    }

    const amountEntry = windows.find((window) => window.remainingAmount != null);
    if (amountEntry && amountEntry.remainingAmount != null) {
        return {
            kind: "amount",
            primaryWindow: amountEntry,
            formattedAmount: formatQuotaAmount(amountEntry.remainingAmount, amountEntry.currency),
            colorByIndex,
            hasDepleted: amountEntry.remainingAmount <= 0,
        };
    }

    return { kind: "empty", colorByIndex, hasDepleted: false };
}

const MAX_RINGS_PER_CLUSTER = 5;

function ringGeometry(count: number): { strokeWidth: number; radii: number[] } {
    switch (count) {
        case 1:
            return { strokeWidth: 3.6, radii: [12.4] };
        case 2:
            return { strokeWidth: 3.1, radii: [12.8, 8.4] };
        case 3:
            return { strokeWidth: 2.6, radii: [13.2, 9.6, 6.0] };
        case 4:
            return { strokeWidth: 2.1, radii: [13.5, 10.6, 7.7, 4.8] };
        default:
            return { strokeWidth: 1.75, radii: [13.8, 11.4, 9.0, 6.6, 4.2] };
    }
}

export interface QuotaRingsChartProps {
    clusters: QuotaRingCluster[];
    size?: number;
    className?: string;
}

export const QuotaRingsChart: React.FC<QuotaRingsChartProps> = ({
    clusters,
    size = 24,
    className,
}) => {
    return (
        <span className={cn("inline-flex items-center gap-1", className)}>
            {clusters.map((cluster, clusterIdx) => {
                const visibleRings = cluster.rings.slice(0, MAX_RINGS_PER_CLUSTER);
                const overflow = cluster.rings.length - visibleRings.length;
                const { strokeWidth, radii } = ringGeometry(visibleRings.length);

                return (
                    <span
                        key={cluster.group ?? `cluster-${clusterIdx}`}
                        className="inline-flex items-center gap-0.5"
                    >
                        <svg
                            width={size}
                            height={size}
                            viewBox="0 0 32 32"
                            aria-hidden="true"
                            data-testid="quota-ring-cluster"
                            data-ring-count={visibleRings.length}
                            className="shrink-0 overflow-visible"
                        >
                            <g transform="rotate(-90 16 16)">
                                {visibleRings.map((ring, idx) => {
                                    const r = radii[idx];
                                    const circumference = 2 * Math.PI * r;
                                    const dashOffset = circumference * (1 - ring.percent / 100);
                                    return (
                                        <g
                                            key={`${ring.label}-${ring.windowIndex}`}
                                            data-testid="quota-ring"
                                            data-ring-label={ring.label}
                                            data-ring-percent={ring.percent}
                                        >
                                            <circle
                                                cx={16}
                                                cy={16}
                                                r={r}
                                                fill="none"
                                                stroke={ring.color}
                                                strokeOpacity={0.2}
                                                strokeWidth={strokeWidth}
                                            />
                                            {ring.percent > 0 && (
                                                <circle
                                                    cx={16}
                                                    cy={16}
                                                    r={r}
                                                    fill="none"
                                                    stroke={ring.color}
                                                    strokeWidth={strokeWidth}
                                                    strokeLinecap="round"
                                                    strokeDasharray={circumference}
                                                    strokeDashoffset={dashOffset}
                                                    className="transition-[stroke-dashoffset] duration-300 ease-out"
                                                />
                                            )}
                                        </g>
                                    );
                                })}
                            </g>
                        </svg>
                        {overflow > 0 && (
                            <span className="text-[10px] font-medium tabular-nums text-muted-foreground">
                                +{overflow}
                            </span>
                        )}
                    </span>
                );
            })}
        </span>
    );
};

interface QuotaWindowRowProps {
    window: QuotaWindow;
    color?: string;
    compact?: boolean;
}

function QuotaWindowRow({ window, color, compact = false }: QuotaWindowRowProps) {
    const percent = window.remainingPercent;
    const remaining = formatWindowRemaining(window);
    const depleted = (percent != null && percent <= 0) || (percent == null && window.remainingAmount != null && window.remainingAmount <= 0);

    return (
        <div className={compact ? "space-y-1" : "space-y-1.5"}>
            <div className="flex items-center justify-between gap-3 text-xs sm:text-[13px]">
                <span className="flex min-w-0 items-center gap-2 text-foreground">
                    {color && (
                        <span
                            aria-hidden="true"
                            className="h-2 w-2 shrink-0 rounded-full"
                            style={{ backgroundColor: color }}
                        />
                    )}
                    <span className="truncate">{window.label}</span>
                </span>
                <span
                    className={cn(
                        "shrink-0 font-medium tabular-nums",
                        depleted ? "text-destructive" : "text-foreground",
                    )}
                >
                    {remaining}
                </span>
            </div>
            {percent != null && (
                <div className={cn("overflow-hidden rounded-full bg-muted/80", compact ? "h-1.5" : "h-1.5")}>
                    <div
                        className="h-full rounded-full transition-[width] duration-300 ease-out"
                        style={{
                            width: `${Math.max(0, Math.min(100, percent))}%`,
                            backgroundColor: color ?? "hsl(var(--primary))",
                        }}
                    />
                </div>
            )}
            <p className="text-[11px] text-muted-foreground">
                {window.resetsAt ? (
                    <>
                        重置于 <time dateTime={window.resetsAt}>{new Date(window.resetsAt).toLocaleString()}</time>
                    </>
                ) : (
                    "服务未提供重置时间"
                )}
            </p>
        </div>
    );
}

export interface QuotaDetailsListProps {
    windows: QuotaWindow[];
    colorByIndex: Map<number, string>;
    compact?: boolean;
}

export const QuotaDetailsList: React.FC<QuotaDetailsListProps> = ({
    windows,
    colorByIndex,
    compact = false,
}) => {
    const groups = new Map<string | undefined, Array<{ window: QuotaWindow; index: number }>>();
    windows.forEach((window, index) => {
        const existing = groups.get(window.group);
        if (existing) existing.push({ window, index });
        else groups.set(window.group, [{ window, index }]);
    });

    return (
        <div className={compact ? "space-y-2.5" : "space-y-3"}>
            {Array.from(groups, ([name, items]) => {
                const rows = items.map(({ window, index }) => (
                    <QuotaWindowRow
                        key={`${window.label}-${index}`}
                        window={window}
                        color={colorByIndex.get(index)}
                        compact={compact}
                    />
                ));
                return name ? (
                    <section key={name} aria-label={name} className={compact ? "space-y-2" : "space-y-2.5"}>
                        <h4 className="text-xs font-medium text-muted-foreground">{name}</h4>
                        <div className={compact ? "space-y-2" : "space-y-2.5"}>{rows}</div>
                    </section>
                ) : (
                    <div key="" className={compact ? "space-y-2" : "space-y-2.5"}>
                        {rows}
                    </div>
                );
            })}
        </div>
    );
};
