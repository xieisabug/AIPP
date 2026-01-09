import { Progress } from "@/components/ui/progress";
import { Card, CardContent } from "@/components/ui/card";

interface TokenUsageDisplayProps {
    total: number;
    input: number;
    output: number;
    showPercentage?: boolean;
    compact?: boolean;
}

export function TokenUsageDisplay({
    total,
    input,
    output,
    showPercentage = false,
    compact = false,
}: TokenUsageDisplayProps) {
    const inputPercentage = total > 0 ? (input / total) * 100 : 0;
    const outputPercentage = total > 0 ? (output / total) * 100 : 0;

    const formatNumber = (num: number) => {
        return new Intl.NumberFormat("en-US").format(num);
    };

    if (compact) {
        return (
            <div className="flex items-center gap-4 text-sm">
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
                </div>
            </CardContent>
        </Card>
    );
}
