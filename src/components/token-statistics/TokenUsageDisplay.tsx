import { Progress } from "@/components/ui/progress";
import { Card, CardContent } from "@/components/ui/card";

interface TokenUsageDisplayProps {
    total: number;
    input: number;
    output: number;
    thought?: number;
    cachedRead?: number;
    cachedWrite?: number;
    showPercentage?: boolean;
    compact?: boolean;
}

export function TokenUsageDisplay({
    total,
    input,
    output,
    thought = 0,
    cachedRead = 0,
    cachedWrite = 0,
    showPercentage = false,
    compact = false,
}: TokenUsageDisplayProps) {
    const inputPercentage = total > 0 ? (input / total) * 100 : 0;
    const outputPercentage = total > 0 ? (output / total) * 100 : 0;

    const formatNumber = (num: number) => {
        return new Intl.NumberFormat("en-US").format(num);
    };

    const extraRows = [
        { label: "思考 Token", value: thought },
        { label: "缓存读取", value: cachedRead },
        { label: "缓存写入", value: cachedWrite },
    ].filter((item) => item.value > 0);

    if (compact) {
        return (
            <div className="space-y-3 text-sm">
                <div className="flex items-center gap-4">
                    <div className="flex-1">
                        <div className="flex justify-between mb-1">
                            <span className="text-muted-foreground">输入</span>
                            <span className="font-medium">{formatNumber(input)}</span>
                        </div>
                        <Progress value={inputPercentage} className="h-2" />
                    </div>
                    <div className="flex-1">
                        <div className="flex justify-between mb-1">
                            <span className="text-muted-foreground">输出</span>
                            <span className="font-medium">{formatNumber(output)}</span>
                        </div>
                        <Progress value={outputPercentage} className="h-2" />
                    </div>
                </div>
                {extraRows.length > 0 && (
                    <div className="grid grid-cols-3 gap-3 text-xs">
                        {extraRows.map((row) => (
                            <div key={row.label} className="rounded-md bg-muted/40 px-2 py-1">
                                <div className="text-muted-foreground">{row.label}</div>
                                <div className="font-medium">{formatNumber(row.value)}</div>
                            </div>
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
                    {/* Total Tokens */}
                    <div className="text-center pb-4 border-b">
                        <p className="text-sm text-muted-foreground mb-1">总 Token</p>
                        <p className="text-3xl font-bold">{formatNumber(total)}</p>
                    </div>

                    {/* Input Tokens */}
                    <div>
                        <div className="flex justify-between mb-2">
                            <span className="text-sm text-muted-foreground">输入 Token</span>
                            <span className="text-sm font-medium">
                                {formatNumber(input)}
                                {showPercentage && (
                                    <span className="text-muted-foreground ml-1">
                                        ({inputPercentage.toFixed(1)}%)
                                    </span>
                                )}
                            </span>
                        </div>
                        <Progress value={inputPercentage} className="h-2" />
                    </div>

                    {/* Output Tokens */}
                    <div>
                        <div className="flex justify-between mb-2">
                            <span className="text-sm text-muted-foreground">输出 Token</span>
                            <span className="text-sm font-medium">
                                {formatNumber(output)}
                                {showPercentage && (
                                    <span className="text-muted-foreground ml-1">
                                        ({outputPercentage.toFixed(1)}%)
                                    </span>
                                )}
                            </span>
                        </div>
                        <Progress value={outputPercentage} className="h-2" />
                    </div>

                    {extraRows.length > 0 && (
                        <div className="space-y-2 border-t pt-4">
                            {extraRows.map((row) => (
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
