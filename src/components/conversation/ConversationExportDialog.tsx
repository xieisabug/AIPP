import { useState, useEffect, useCallback } from "react";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogHeader,
    DialogTitle,
    DialogTrigger,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import IconButton from "../IconButton";
import Download from "../../assets/download.svg?react";
import { conversationExportService } from "@/services/conversationExportService";
import type { ConversationExportOptions } from "@/utils/exportFormatters";
import type { ExportData } from "@/utils/exportFormatters";
import { Loader2, FileText, FileImage, File } from "lucide-react";

interface ConversationExportDialogProps {
    conversationId: string;
}

const defaultOptions: ConversationExportOptions = {
    includeSystemPrompt: true,
    includeReasoning: true,
    includeToolParams: true,
    includeToolResults: true,
};

const ConversationExportDialog: React.FC<ConversationExportDialogProps> = ({
    conversationId,
}) => {
    const [open, setOpen] = useState(false);
    const [loading, setLoading] = useState(false);
    const [exporting, setExporting] = useState<string | null>(null);
    const [exportData, setExportData] = useState<ExportData | null>(null);
    const [options, setOptions] = useState<ConversationExportOptions>(defaultOptions);

    // 当对话框打开时获取导出数据
    useEffect(() => {
        if (open && conversationId) {
            setLoading(true);
            conversationExportService
                .getExportData(conversationId)
                .then((data) => {
                    setExportData(data);
                })
                .catch((error) => {
                    console.error("Failed to load export data:", error);
                })
                .finally(() => {
                    setLoading(false);
                });
        }
    }, [open, conversationId]);

    // 更新单个选项
    const updateOption = useCallback(
        (key: keyof ConversationExportOptions, value: boolean) => {
            setOptions((prev: ConversationExportOptions) => ({ ...prev, [key]: value }));
        },
        [],
    );

    // 获取对话名称作为默认文件名
    const getDefaultFilename = useCallback(() => {
        if (!exportData) return "conversation";
        return exportData.conversation.conversation.name;
    }, [exportData]);

    // 导出处理函数
    const handleExport = useCallback(
        async (format: "markdown" | "pdf" | "png") => {
            if (!exportData) return;

            setExporting(format);
            try {
                const filename = getDefaultFilename();

                switch (format) {
                    case "markdown":
                        await conversationExportService.exportToMarkdown(
                            exportData,
                            options,
                            filename,
                        );
                        break;
                    case "pdf":
                        await conversationExportService.exportToPDF(
                            exportData,
                            options,
                            filename,
                        );
                        break;
                    case "png":
                        await conversationExportService.exportToPNG(
                            exportData,
                            options,
                            filename,
                        );
                        break;
                }
            } catch (error) {
                console.error("Export failed:", error);
            } finally {
                setExporting(null);
            }
        },
        [exportData, options, getDefaultFilename],
    );

    const isExporting = exporting !== null;

    return (
        <Dialog open={open} onOpenChange={setOpen}>
            <DialogTrigger asChild>
                <IconButton
                    icon={<Download className="fill-foreground" />}
                    onClick={() => {}}
                    border
                />
            </DialogTrigger>
            <DialogContent className="max-w-md">
                <DialogHeader>
                    <DialogTitle>导出对话</DialogTitle>
                    <DialogDescription>
                        选择要包含的内容并选择导出格式
                    </DialogDescription>
                </DialogHeader>

                <div className="space-y-4 py-4">
                    {/* 导出选项 */}
                    <div className="space-y-3">
                        <h4 className="text-sm font-medium">导出选项</h4>

                        <div className="flex items-center space-x-2">
                            <Checkbox
                                id="include-system"
                                checked={options.includeSystemPrompt}
                                onCheckedChange={(checked) =>
                                    updateOption("includeSystemPrompt", checked as boolean)
                                }
                                disabled={loading || isExporting}
                            />
                            <Label
                                htmlFor="include-system"
                                className="text-sm cursor-pointer"
                            >
                                包含系统提示词
                            </Label>
                        </div>

                        <div className="flex items-center space-x-2">
                            <Checkbox
                                id="include-reasoning"
                                checked={options.includeReasoning}
                                onCheckedChange={(checked) =>
                                    updateOption("includeReasoning", checked as boolean)
                                }
                                disabled={loading || isExporting}
                            />
                            <Label
                                htmlFor="include-reasoning"
                                className="text-sm cursor-pointer"
                            >
                                包含推理过程
                            </Label>
                        </div>

                        <div className="flex items-center space-x-2">
                            <Checkbox
                                id="include-tool-params"
                                checked={options.includeToolParams}
                                onCheckedChange={(checked) =>
                                    updateOption("includeToolParams", checked as boolean)
                                }
                                disabled={loading || isExporting}
                            />
                            <Label
                                htmlFor="include-tool-params"
                                className="text-sm cursor-pointer"
                            >
                                包含工具调用参数
                            </Label>
                        </div>

                        <div className="flex items-center space-x-2">
                            <Checkbox
                                id="include-tool-results"
                                checked={options.includeToolResults}
                                onCheckedChange={(checked) =>
                                    updateOption("includeToolResults", checked as boolean)
                                }
                                disabled={loading || isExporting}
                            />
                            <Label
                                htmlFor="include-tool-results"
                                className="text-sm cursor-pointer"
                            >
                                包含工具执行结果
                            </Label>
                        </div>
                    </div>

                    {/* 加载状态 */}
                    {loading && (
                        <div className="flex items-center justify-center py-4">
                            <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
                            <span className="ml-2 text-sm text-muted-foreground">
                                加载中...
                            </span>
                        </div>
                    )}

                    {/* 导出按钮 */}
                    {!loading && (
                        <div className="space-y-3 pt-2">
                            <h4 className="text-sm font-medium">导出格式</h4>
                            <div className="grid grid-cols-3 gap-2">
                                <Button
                                    variant="outline"
                                    onClick={() => handleExport("markdown")}
                                    disabled={isExporting}
                                    className="flex flex-col items-center gap-1 h-auto py-3"
                                >
                                    {exporting === "markdown" ? (
                                        <Loader2 className="h-4 w-4 animate-spin" />
                                    ) : (
                                        <FileText className="h-4 w-4" />
                                    )}
                                    <span className="text-xs">Markdown</span>
                                </Button>

                                <Button
                                    variant="outline"
                                    onClick={() => handleExport("pdf")}
                                    disabled={isExporting}
                                    className="flex flex-col items-center gap-1 h-auto py-3"
                                >
                                    {exporting === "pdf" ? (
                                        <Loader2 className="h-4 w-4 animate-spin" />
                                    ) : (
                                        <File className="h-4 w-4" />
                                    )}
                                    <span className="text-xs">PDF</span>
                                </Button>

                                <Button
                                    variant="outline"
                                    onClick={() => handleExport("png")}
                                    disabled={isExporting}
                                    className="flex flex-col items-center gap-1 h-auto py-3"
                                >
                                    {exporting === "png" ? (
                                        <Loader2 className="h-4 w-4 animate-spin" />
                                    ) : (
                                        <FileImage className="h-4 w-4" />
                                    )}
                                    <span className="text-xs">图片</span>
                                </Button>
                            </div>
                        </div>
                    )}
                </div>
            </DialogContent>
        </Dialog>
    );
};

export default ConversationExportDialog;
