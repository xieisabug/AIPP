import { useState, useCallback } from "react";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogHeader,
    DialogTitle,
    DialogTrigger,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import IconButton from "../IconButton";
import { conversationExportService } from "@/services/conversationExportService";
import { Loader2, FileText, FileImage, File, FileType, Download } from "lucide-react";

interface MessageExportDialogProps {
    messageContent: string;
    messageType: string;
    onOpenChange?: (open: boolean) => void;
}

const MessageExportDialog: React.FC<MessageExportDialogProps> = ({
    messageContent,
    messageType,
    onOpenChange,
}) => {
    const [open, setOpen] = useState(false);
    const [exporting, setExporting] = useState<string | null>(null);

    const handleOpenChange = useCallback(
        (value: boolean) => {
            setOpen(value);
            onOpenChange?.(value);
        },
        [onOpenChange],
    );

    const handleExport = useCallback(
        async (format: "markdown" | "pdf" | "png" | "word") => {
            setExporting(format);
            try {
                let exportSucceeded = false;
                switch (format) {
                    case "markdown":
                        exportSucceeded =
                            await conversationExportService.exportSingleMessageToMarkdown(
                                messageContent,
                                messageType,
                            );
                        break;
                    case "word":
                        exportSucceeded =
                            await conversationExportService.exportSingleMessageToWord(
                                messageContent,
                                messageType,
                            );
                        break;
                    case "pdf":
                        exportSucceeded =
                            await conversationExportService.exportSingleMessageToPDF(
                                messageContent,
                                messageType,
                            );
                        break;
                    case "png":
                        exportSucceeded =
                            await conversationExportService.exportSingleMessageToPNG(
                                messageContent,
                                messageType,
                            );
                        break;
                }
                if (exportSucceeded) {
                    handleOpenChange(false);
                }
            } catch (error) {
                console.error("Export failed:", error);
            } finally {
                setExporting(null);
            }
        },
        [messageContent, messageType, handleOpenChange],
    );

    const isExporting = exporting !== null;

    return (
        <Dialog open={open} onOpenChange={handleOpenChange}>
            <DialogTrigger asChild>
                <IconButton
                    icon={<Download size={16} className="text-icon" />}
                    onClick={() => {}}
                />
            </DialogTrigger>
            <DialogContent className="max-w-xs">
                <DialogHeader>
                    <DialogTitle>导出消息</DialogTitle>
                    <DialogDescription>选择导出格式</DialogDescription>
                </DialogHeader>
                <div className="py-2">
                    <div className="grid grid-cols-4 gap-2">
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
                            onClick={() => handleExport("word")}
                            disabled={isExporting}
                            className="flex flex-col items-center gap-1 h-auto py-3"
                        >
                            {exporting === "word" ? (
                                <Loader2 className="h-4 w-4 animate-spin" />
                            ) : (
                                <FileType className="h-4 w-4" />
                            )}
                            <span className="text-xs">Word</span>
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
            </DialogContent>
        </Dialog>
    );
};

export default MessageExportDialog;
