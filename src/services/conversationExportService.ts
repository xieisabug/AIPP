import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { writeFile } from "@tauri-apps/plugin-fs";
import { toast } from "sonner";
import type {
    ConversationWithMessages,
    ConversationExportOptions,
    MCPToolCall,
} from "@/data/Conversation";
import {
    formatAsMarkdown,
    sanitizeFilename,
    type ExportData,
} from "@/utils/exportFormatters";
import { renderExportContent } from "@/components/conversation/ConversationExportRenderer";

/**
 * 对话导出服务
 */
export const conversationExportService = {
    /**
     * 获取导出数据
     */
    async getExportData(conversationId: string): Promise<ExportData> {
        try {
            const [conversation, toolCalls] = await Promise.all([
                invoke<ConversationWithMessages>("get_conversation_with_messages", {
                    conversationId: parseInt(conversationId),
                }),
                invoke<MCPToolCall[]>("get_mcp_tool_calls_by_conversation", {
                    conversationId: parseInt(conversationId),
                }).catch(() => []), // 如果没有工具调用，返回空数组
            ]);
            return { conversation, toolCalls };
        } catch (error) {
            console.error("Failed to get export data:", error);
            throw error;
        }
    },

    /**
     * 保存文件（通用）
     */
    async saveFile(
        content: Uint8Array,
        defaultName: string,
        filters: Array<{ name: string; extensions: string[] }>,
    ): Promise<void> {
        try {
            const path = await save({
                defaultPath: defaultName,
                filters,
            });
            if (path) {
                await writeFile(path, content);
            } else {
                throw new Error("用户取消了保存操作");
            }
        } catch (error) {
            console.error("Failed to save file:", error);
            throw error;
        }
    },

    /**
     * 导出为 Markdown
     */
    async exportToMarkdown(
        data: ExportData,
        options: ConversationExportOptions,
        filename: string,
    ): Promise<void> {
        try {
            const markdown = formatAsMarkdown(data, options);
            const encoder = new TextEncoder();
            const content = encoder.encode(markdown);
            const sanitizedName = sanitizeFilename(filename);
            await this.saveFile(
                content,
                `${sanitizedName}.md`,
                [{ name: "Markdown", extensions: ["md"] }],
            );
            toast.success("Markdown 导出成功");
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : "导出失败";
            if (errorMessage.includes("取消")) {
                // 用户主动取消，不显示错误
                return;
            }
            toast.error(`Markdown 导出失败: ${errorMessage}`);
            throw error;
        }
    },

    /**
     * 使用 React 组件渲染并转换为 canvas
     */
    async renderToCanvas(
        data: ExportData,
        options: ConversationExportOptions,
        width: number = 800,
    ): Promise<HTMLCanvasElement> {
        const { default: html2canvas } = await import("html2canvas");

        // 检测当前是否为暗色模式
        const isDarkMode = document.documentElement.classList.contains("dark");
        // 使用明确的 RGB 背景色避免 oklch 兼容性问题
        const backgroundColor = isDarkMode ? "#0a0a0b" : "#ffffff";

        // 创建容器，使用固定定位但放在可见区域外
        const container = document.createElement("div");
        container.id = "export-render-container";
        container.style.position = "fixed";
        container.style.left = "-9999px";
        container.style.top = "0";
        container.style.width = `${width}px`;
        container.style.background = backgroundColor;
        container.style.color = isDarkMode ? "#fafafa" : "#0a0a0b";
        container.style.minHeight = "100vh";
        document.body.appendChild(container);

        try {
            // 使用 React 渲染内容
            renderExportContent(container, data, options);

            // 等待 React 渲染完成和代码高亮
            await new Promise((resolve) => setTimeout(resolve, 800));

            // 转换为 canvas，使用明确的背景色
            const canvas = await html2canvas(container, {
                scale: 2, // 提高清晰度
                useCORS: true,
                logging: false,
                backgroundColor: backgroundColor,
            });

            return canvas;
        } finally {
            // 清理容器
            if (container.parentNode) {
                container.parentNode.removeChild(container);
            }
        }
    },

    /**
     * 导出为 PDF
     */
    async exportToPDF(
        data: ExportData,
        options: ConversationExportOptions,
        filename: string,
    ): Promise<void> {
        try {
            const { jsPDF } = await import("jspdf");

            // 渲染为 canvas
            const canvas = await this.renderToCanvas(data, options);

            // 创建 PDF
            const pdf = new jsPDF({
                orientation: "portrait",
                unit: "px",
                format: "a4",
            });

            const imgWidth = 595.28; // A4 宽度
            const imgHeight = (canvas.height * imgWidth) / canvas.width;

            let heightLeft = imgHeight;
            let position = 0;

            // 添加第一页
            pdf.addImage(
                canvas.toDataURL("image/jpeg", 0.95),
                "JPEG",
                0,
                position,
                imgWidth,
                imgHeight,
            );
            heightLeft -= pdf.internal.pageSize.height;

            // 如果内容超过一页，添加更多页面
            while (heightLeft > 0) {
                position = heightLeft - imgHeight;
                pdf.addPage();
                pdf.addImage(
                    canvas.toDataURL("image/jpeg", 0.95),
                    "JPEG",
                    0,
                    position,
                    imgWidth,
                    imgHeight,
                );
                heightLeft -= pdf.internal.pageSize.height;
            }

            // 保存 PDF
            const sanitizedName = sanitizeFilename(filename);
            const pdfBytes = pdf.output("arraybuffer");
            const content = new Uint8Array(pdfBytes);

            await this.saveFile(
                content,
                `${sanitizedName}.pdf`,
                [{ name: "PDF", extensions: ["pdf"] }],
            );

            toast.success("PDF 导出成功");
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : "导出失败";
            if (errorMessage.includes("取消")) {
                return;
            }
            toast.error(`PDF 导出失败: ${errorMessage}`);
            throw error;
        }
    },

    /**
     * 导出为 PNG 图片
     */
    async exportToPNG(
        data: ExportData,
        options: ConversationExportOptions,
        filename: string,
    ): Promise<void> {
        try {
            // 渲染为 canvas
            const canvas = await this.renderToCanvas(data, options);

            // 转换为 PNG blob
            canvas.toBlob(async (blob) => {
                if (!blob) {
                    throw new Error("无法生成图片");
                }

                const arrayBuffer = await blob.arrayBuffer();
                const content = new Uint8Array(arrayBuffer);
                const sanitizedName = sanitizeFilename(filename);

                await this.saveFile(
                    content,
                    `${sanitizedName}.png`,
                    [{ name: "PNG", extensions: ["png"] }],
                );

                toast.success("PNG 导出成功");
            }, "image/png");
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : "导出失败";
            if (errorMessage.includes("取消")) {
                return;
            }
            toast.error(`PNG 导出失败: ${errorMessage}`);
            throw error;
        }
    },
};
