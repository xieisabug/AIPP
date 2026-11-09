import { Progress } from "@/components/ui/progress";
import { Card, CardContent } from "@/components/ui/card";

interface TokenUsageDisplayProps {
    total: number;
    /** 全部输入，已含缓存读写 */
    input: number;
    /** 全部输出，已含思考 */
    output: number;
    thought?: number;
    cachedInput?: number;
    cachedWrite?: number;
    showPercentage?: boolean;
    compact?: boolean;
}

const formatNumber = (num: number) => new Intl.NumberFormat("en-US").format(num);

/** 缓存命中率 = 缓存命中输入 / 全部输入；没有输入时为 null */
export function cacheHitRate(input: number, cachedInput: number): number | null {
    return input > 0 ? (Math.min(cachedInput, input) / input) * 100 : null;
}

export function TokenUsageDisplay({
    total,
    input,
    output,
    thought = 0,
    cachedInput = 0,
    cachedWrite = 0,
    showPercentage = false,
    compact = false,
}: TokenUsageDisplayProps) {
    // 三段互不重叠，合起来就是总量：未命中缓存输入 + 缓存命中输入 + 输出
    const uncachedInput = Math.max(input - cachedInput, 0);
    const percentOfTotal = (value: number) => (total > 0 ? (value / total) * 100 : 0);
    const hitRate = cacheHitRate(input, cachedInput);

    const segments = [
        { label: "未命中缓存输入", value: uncachedInput },
        { label: "缓存命中输入", value: cachedInput },
        { label: "输出", value: output },
    ];
    // 这两项分别包含在上面的段里，不能再相加
    const includedRows = [
        { label: "其中缓存写入（含在未命中缓存输入里）", value: cachedWrite },
        { label: "其中思考（含在输出里）", value: thought },
    ].filter((item) => item.value > 0);

    if (compact) {
        return (
            <div className="space-y-3 text-sm">
                <div className="grid grid-cols-3 gap-4">
                    {segments.map((segment) => (
                        <div key={segment.label} className="flex-1">
                            <div className="mb-1 flex justify-between gap-2">
                                <span className="truncate text-muted-foreground">{segment.label}</span>
                                <span className="font-medium">{formatNumber(segment.value)}</span>
                            </div>
                            <Progress value={percentOfTotal(segment.value)} className="h-2" />
                        </div>
                    ))}
                </div>
                {(includedRows.length > 0 || hitRate !== null) && (
                    <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
                        {hitRate !== null && <span>缓存命中率 {hitRate.toFixed(1)}%</span>}
                        {includedRows.map((row) => (
                            <span key={row.label}>
                                {row.label} {formatNumber(row.value)}
                            </span>
                        ))}
                    </div>
                )}
            </div>
        );
    }

    return (
        <Card>
            <CardContent className="pt-6">
                <div className="space-y-4">
                    <div className="text-center pb-4 border-b">
                        <p className="text-sm text-muted-foreground mb-1">总 Token</p>
                        <p className="text-3xl font-bold">{formatNumber(total)}</p>
                        <p className="mt-1 text-xs text-muted-foreground">
                            输入 {formatNumber(input)} + 输出 {formatNumber(output)}
                        </p>
                    </div>

                    {segments.map((segment) => (
                        <div key={segment.label}>
                            <div className="flex justify-between mb-2">
                                <span className="text-sm text-muted-foreground">{segment.label}</span>
                                <span className="text-sm font-medium">
                                    {formatNumber(segment.value)}
                                    {showPercentage && (
                                        <span className="text-muted-foreground ml-1">
                                            ({percentOfTotal(segment.value).toFixed(1)}%)
                                        </span>
                                    )}
                                </span>
                            </div>
                            <Progress value={percentOfTotal(segment.value)} className="h-2" />
                        </div>
                    ))}

                    {(includedRows.length > 0 || hitRate !== null) && (
                        <div className="space-y-2 border-t pt-4">
                            {hitRate !== null && (
                                <div className="flex justify-between text-sm">
                                    <span className="text-muted-foreground">缓存命中率（缓存命中输入 / 全部输入）</span>
                                    <span className="font-medium">{hitRate.toFixed(1)}%</span>
                                </div>
                            )}
                            {includedRows.map((row) => (
                                <div key={row.label} className="flex justify-between text-sm">
                                    <span className="text-muted-foreground">{row.label}</span>
                                    <span className="font-medium">{formatNumber(row.value)}</span>
                                </div>
                            ))}
                        </div>
                    )}
                </div>
            </CardContent>
        </Card>
    );
}
