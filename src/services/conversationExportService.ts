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
    stripMcpToolCallMarkers,
    extractMcpToolCallHints,
    formatJsonContent,
    getLatestBranchMessages,
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
            const filteredMessages = getLatestBranchMessages(conversation.messages);
            const messageIdSet = new Set(filteredMessages.map((msg) => msg.id));
            const hintCallIdSet = new Set<number>();
            for (const message of filteredMessages) {
                const hints = extractMcpToolCallHints(message.content || "");
                for (const hint of hints) {
                    if (typeof hint.call_id === "number") {
                        hintCallIdSet.add(hint.call_id);
                    }
                }
            }
            const filteredToolCalls = toolCalls.filter((call) => {
                if (typeof call.message_id === "number" && messageIdSet.has(call.message_id)) {
                    return true;
                }
                return hintCallIdSet.has(call.id);
            });
            return {
                conversation: {
                    ...conversation,
                    messages: filteredMessages,
                },
                toolCalls: filteredToolCalls,
            };
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
     * 渲染 PDF 专用内容到 canvas（简洁文档样式，无气泡）
     * 按消息分段渲染，支持智能分页
     */
    async renderPdfToCanvas(
        data: ExportData,
        options: ConversationExportOptions,
    ): Promise<{ canvases: HTMLCanvasElement[], heights: number[] }> {
        const { default: html2canvas } = await import("html2canvas");

        // PDF 使用 A4 宽度比例
        const width = 595;
        const backgroundColor = "#ffffff";

        const canvases: HTMLCanvasElement[] = [];
        const heights: number[] = [];

        // 创建头部容器
        const headerContainer = document.createElement("div");
        headerContainer.style.position = "fixed";
        headerContainer.style.left = "-9999px";
        headerContainer.style.top = "0";
        headerContainer.style.width = `${width}px`;
        headerContainer.style.background = backgroundColor;
        headerContainer.style.padding = "24px";
        headerContainer.style.fontFamily = '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, "Microsoft YaHei", sans-serif';
        document.body.appendChild(headerContainer);

        const formatDate = (date: Date) => {
            return new Date(date).toLocaleString("zh-CN", {
                year: "numeric",
                month: "2-digit",
                day: "2-digit",
                hour: "2-digit",
                minute: "2-digit",
            });
        };

        // 渲染头部
        headerContainer.innerHTML = `
            <div style="padding-bottom: 12px; border-bottom: 2px solid #111;">
                <h1 style="font-size: 18px; font-weight: 600; margin: 0 0 6px 0; color: #111;">${this.escapeHtml(data.conversation.conversation.name)}</h1>
                <p style="font-size: 11px; color: #666; margin: 0;">
                    助手: ${this.escapeHtml(data.conversation.conversation.assistant_name)} | 
                    创建时间: ${formatDate(new Date(data.conversation.conversation.created_time))}
                </p>
            </div>
        `;

        await new Promise((resolve) => setTimeout(resolve, 100));
        const headerCanvas = await html2canvas(headerContainer, {
            scale: 2,
            useCORS: true,
            logging: false,
            backgroundColor: backgroundColor,
        });
        canvases.push(headerCanvas);
        heights.push(headerCanvas.height / 2); // 因为 scale: 2

        document.body.removeChild(headerContainer);

        // 过滤消息
        const messages = data.conversation.messages.filter((msg) => {
            if (msg.message_type === "tool_result") return false;
            if (msg.message_type === "user" && msg.content?.startsWith("Tool execution results:\n")) return false;
            if (msg.message_type === "system") return options.includeSystemPrompt;
            if (msg.message_type === "reasoning") return options.includeReasoning;
            return true;
        });

        // 构建工具调用映射
        const toolCallMap = new Map<number, typeof data.toolCalls>();
        for (const tc of data.toolCalls) {
            if (tc.message_id) {
                if (!toolCallMap.has(tc.message_id)) {
                    toolCallMap.set(tc.message_id, []);
                }
                toolCallMap.get(tc.message_id)!.push(tc);
            }
        }
        const toolCallById = new Map<number, (typeof data.toolCalls)[number]>();
        for (const tc of data.toolCalls) {
            toolCallById.set(tc.id, tc);
        }

        // 逐条消息渲染
        for (const message of messages) {
            const msgContainer = document.createElement("div");
            msgContainer.style.position = "fixed";
            msgContainer.style.left = "-9999px";
            msgContainer.style.top = "0";
            msgContainer.style.width = `${width}px`;
            msgContainer.style.background = backgroundColor;
            msgContainer.style.padding = "0 24px";
            msgContainer.style.fontFamily = '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, "Microsoft YaHei", sans-serif';
            document.body.appendChild(msgContainer);

            const label = this.getMessageLabel(message.message_type);
            const labelStyle = this.getMessageLabelStyle(message.message_type);
            let toolCallsHtml = this.generateToolCallsHtml(message, options, toolCallMap);

            if (options.includeToolParams && (!message.tool_calls_json || message.tool_calls_json.trim() === "") && toolCallMap.has(message.id)) {
                const mappedCalls = toolCallMap.get(message.id) || [];
                const paramBlocks = mappedCalls.map((tc) => {
                    return `
                            <div style="margin-top: 8px; padding: 8px; background: #f9f9f9; border-left: 3px solid #2563eb; font-size: 11px;">
                                <div style="font-weight: 500; margin-bottom: 4px; color: #333;">🔧 ${this.escapeHtml(tc.server_name)} / ${this.escapeHtml(tc.tool_name)}</div>
                                <pre style="background: #f0f0f0; padding: 6px; border-radius: 3px; margin: 0; overflow-x: auto;"><code style="font-family: Consolas, Monaco, monospace; font-size: 10px; color: #333; white-space: pre-wrap; word-break: break-word;">${this.escapeHtml(formatJsonContent(tc.parameters))}</code></pre>
                            </div>
                    `;
                }).join("");
                toolCallsHtml += paramBlocks;
            }

            if (options.includeToolResults && toolCallMap.has(message.id)) {
                const relatedCalls = (toolCallMap.get(message.id) || []).filter(
                    (call) => call.status === "success" && call.result,
                );
                if (relatedCalls.length > 0) {
                    const resultBlocks = relatedCalls.map((tc) => {
                        const statusText = tc.status === "success" ? "✓" : tc.status === "failed" ? "✗" : "...";
                        let resultHtml = "";
                        if (tc.result) {
                            resultHtml = `<pre style="background: #f0f0f0; padding: 6px; border-radius: 3px; margin: 4px 0 0; overflow-x: auto;"><code style="font-family: Consolas, Monaco, monospace; font-size: 10px; color: #333; white-space: pre-wrap; word-break: break-word;">${this.escapeHtml(formatJsonContent(tc.result))}</code></pre>`;
                        }
                        return `
                            <div style="margin-top: 8px; padding: 8px; background: #f9f9f9; border-left: 3px solid ${tc.status === "success" ? "#22c55e" : tc.status === "failed" ? "#dc2626" : "#666"}; font-size: 11px;">
                                <div style="font-weight: 500; margin-bottom: 4px; color: #333;">${statusText} ${this.escapeHtml(tc.server_name)} / ${this.escapeHtml(tc.tool_name)}</div>
                                ${resultHtml}
                            </div>
                        `;
                    }).join("");
                    toolCallsHtml += resultBlocks;
                }
            }

            msgContainer.innerHTML = `
                <div style="padding: 16px 0; border-bottom: 2px solid #e0e0e0;">
                    <div style="${labelStyle}">${this.escapeHtml(label)}</div>
                    <div style="color: #111; font-size: 12px; line-height: 1.6; margin-top: 8px;">${this.markdownToHtml(stripMcpToolCallMarkers(message.content || ""))}</div>
                    ${toolCallsHtml}
                </div>
            `;

            await new Promise((resolve) => setTimeout(resolve, 50));
            const msgCanvas = await html2canvas(msgContainer, {
                scale: 2,
                useCORS: true,
                logging: false,
                backgroundColor: backgroundColor,
            });
            canvases.push(msgCanvas);
            heights.push(msgCanvas.height / 2);

            document.body.removeChild(msgContainer);
        }

        return { canvases, heights };
    },

    // HTML 转义辅助函数
    escapeHtml(str: string): string {
        return str
            .replace(/&/g, "&amp;")
            .replace(/</g, "&lt;")
            .replace(/>/g, "&gt;")
            .replace(/"/g, "&quot;")
            .replace(/'/g, "&#39;");
    },

    // 获取消息标签
    getMessageLabel(messageType: string): string {
        const labels: Record<string, string> = {
            system: "📋 系统提示",
            user: "👤 用户",
            assistant: "🤖 助手",
            reasoning: "💭 推理过程",
            response: "💬 回复",
            error: "⚠️ 错误",
        };
        return labels[messageType] || messageType;
    },

    // 获取消息标签样式
    getMessageLabelStyle(messageType: string): string {
        const baseStyle = "font-size: 13px; font-weight: 700; padding: 4px 10px; border-radius: 4px; display: inline-block;";
        const styles: Record<string, string> = {
            system: `${baseStyle} background: #f0f0f0; color: #666; border-left: 4px solid #888;`,
            user: `${baseStyle} background: #e3f2fd; color: #1565c0; border-left: 4px solid #1976d2;`,
            assistant: `${baseStyle} background: #e8f5e9; color: #2e7d32; border-left: 4px solid #43a047;`,
            reasoning: `${baseStyle} background: #fff3e0; color: #e65100; border-left: 4px solid #ff9800;`,
            response: `${baseStyle} background: #e8f5e9; color: #2e7d32; border-left: 4px solid #43a047;`,
            error: `${baseStyle} background: #ffebee; color: #c62828; border-left: 4px solid #e53935;`,
        };
        return styles[messageType] || `${baseStyle} background: #f5f5f5; color: #333; border-left: 4px solid #999;`;
    },

    // Markdown 转 HTML
    markdownToHtml(md: string): string {
        let html = this.escapeHtml(md);
        
        // 代码块
        html = html.replace(/```(\w*)\n([\s\S]*?)```/g, (_match, _lang, code) => {
            return `<pre style="background: #f5f5f5; padding: 10px; border-radius: 4px; margin: 8px 0; border: 1px solid #e0e0e0; overflow-x: auto;"><code style="font-family: Consolas, Monaco, monospace; font-size: 11px; color: #333; white-space: pre-wrap; word-break: break-word;">${code}</code></pre>`;
        });
        
        html = html.replace(/`([^`]+)`/g, '<code style="background: #f5f5f5; padding: 1px 4px; border-radius: 3px; font-family: Consolas, Monaco, monospace; font-size: 0.9em; color: #333;">$1</code>');
        
        html = html.replace(/^### (.*$)/gm, '<h4 style="font-size: 13px; font-weight: 600; margin: 10px 0 5px; color: #111;">$1</h4>');
        html = html.replace(/^## (.*$)/gm, '<h3 style="font-size: 14px; font-weight: 600; margin: 10px 0 5px; color: #111;">$1</h3>');
        html = html.replace(/^# (.*$)/gm, '<h2 style="font-size: 15px; font-weight: 600; margin: 10px 0 5px; color: #111;">$1</h2>');
        
        html = html.replace(/\*\*\*(.+?)\*\*\*/g, '<strong><em>$1</em></strong>');
        html = html.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
        html = html.replace(/\*(.+?)\*/g, '<em>$1</em>');
        
        html = html.replace(/^\s*[-*+]\s+(.*)$/gm, '<li style="margin: 2px 0;">$1</li>');
        html = html.replace(/(<li[^>]*>.*<\/li>\n?)+/g, '<ul style="margin: 5px 0; padding-left: 18px;">$&</ul>');
        html = html.replace(/^\s*\d+\.\s+(.*)$/gm, '<li style="margin: 2px 0;">$1</li>');
        
        html = html.replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2" style="color: #2563eb; text-decoration: underline;">$1</a>');
        
        html = html.replace(/\n\n/g, '</p><p style="margin: 5px 0;">');
        html = html.replace(/\n/g, '<br>');
        
        return `<p style="margin: 5px 0;">${html}</p>`;
    },

    // 生成工具调用 HTML
    generateToolCallsHtml(
        message: any,
        options: ConversationExportOptions,
        toolCallMap: Map<number, any[]>,
    ): string {
        let html = "";

        if (options.includeToolParams && message.tool_calls_json) {
            try {
                const parsedCalls = JSON.parse(message.tool_calls_json);
                if (Array.isArray(parsedCalls) && parsedCalls.length > 0) {
                    html += parsedCalls.map((tc: any) => {
                        const parts = tc.fn_name?.split("__") || [];
                        const toolName = parts.length > 1 ? parts.slice(1).join("__") : tc.fn_name;
                        const serverName = parts[0] || "unknown";
                        return `
                            <div style="margin-top: 8px; padding: 8px; background: #f9f9f9; border-left: 3px solid #2563eb; font-size: 11px;">
                                <div style="font-weight: 500; margin-bottom: 4px; color: #333;">🔧 ${this.escapeHtml(serverName)} / ${this.escapeHtml(toolName)}</div>
                                <pre style="background: #f0f0f0; padding: 6px; border-radius: 3px; margin: 0; overflow-x: auto;"><code style="font-family: Consolas, Monaco, monospace; font-size: 10px; color: #333; white-space: pre-wrap; word-break: break-word;">${this.escapeHtml(JSON.stringify(tc.fn_arguments, null, 2))}</code></pre>
                            </div>
                        `;
                    }).join("");
                }
            } catch { /* ignore */ }
        }

        if (options.includeToolResults && toolCallMap.has(message.id)) {
            const relatedCalls = toolCallMap.get(message.id);
            if (relatedCalls && relatedCalls.length > 0) {
                html += relatedCalls.map((tc: any) => {
                    const statusText = tc.status === "success" ? "✓" : tc.status === "failed" ? "✗" : "...";
                    let resultHtml = "";
                    if (tc.status === "success" && tc.result) {
                        resultHtml = `<pre style="background: #f0f0f0; padding: 6px; border-radius: 3px; margin: 4px 0 0; overflow-x: auto;"><code style="font-family: Consolas, Monaco, monospace; font-size: 10px; color: #333; white-space: pre-wrap; word-break: break-word;">${this.escapeHtml(formatJsonContent(tc.result))}</code></pre>`;
                    } else if (tc.status === "failed" && tc.error) {
                        resultHtml = `<div style="color: #dc2626; font-size: 10px; margin-top: 4px;">错误: ${this.escapeHtml(tc.error)}</div>`;
                    }
                    return `
                        <div style="margin-top: 8px; padding: 8px; background: #f9f9f9; border-left: 3px solid ${tc.status === "success" ? "#22c55e" : tc.status === "failed" ? "#dc2626" : "#666"}; font-size: 11px;">
                            <div style="font-weight: 500; margin-bottom: 4px; color: #333;">${statusText} ${this.escapeHtml(tc.server_name)} / ${this.escapeHtml(tc.tool_name)}</div>
                            ${resultHtml}
                        </div>
                    `;
                }).join("");
            }
        }

        return html;
    },

    /**
     * 导出为 PDF（智能分页，不截断消息）
     */
    async exportToPDF(
        data: ExportData,
        options: ConversationExportOptions,
        filename: string,
    ): Promise<void> {
        try {
            const { jsPDF } = await import("jspdf");

            // 分段渲染
            const { canvases } = await this.renderPdfToCanvas(data, options);

            // 创建 PDF
            const pdf = new jsPDF({
                orientation: "portrait",
                unit: "pt",
                format: "a4",
            });

            const pageWidth = pdf.internal.pageSize.getWidth();
            const pageHeight = pdf.internal.pageSize.getHeight();
            const margin = 20;

            let currentY = margin;

            for (let i = 0; i < canvases.length; i++) {
                const canvas = canvases[i];
                const imgWidth = pageWidth - margin * 2;
                const imgHeight = (canvas.height * imgWidth) / canvas.width;

                // 计算当前页剩余空间
                const remainingSpace = pageHeight - margin - currentY;

                // 如果当前页已经没有空间了（剩余不足10pt），先换页
                if (remainingSpace < 10) {
                    pdf.addPage();
                    currentY = margin;
                }

                // 如果内容能完整放入当前页剩余空间
                if (imgHeight <= pageHeight - margin - currentY) {
                    pdf.addImage(
                        canvas.toDataURL("image/jpeg", 0.95),
                        "JPEG",
                        margin,
                        currentY,
                        imgWidth,
                        imgHeight,
                    );
                    currentY += imgHeight;
                } else {
                    // 内容需要跨页，进行分割
                    let remainingImgHeight = imgHeight;
                    let sourceY = 0;

                    while (remainingImgHeight > 0) {
                        // 计算当前页可用的空间
                        const availableHeight = pageHeight - margin - currentY;
                        
                        // 如果当前页没有空间了，换页
                        if (availableHeight < 10) {
                            pdf.addPage();
                            currentY = margin;
                            continue;
                        }

                        // 计算这一页能放多少
                        const sliceHeight = Math.min(remainingImgHeight, availableHeight);
                        const sourceHeight = (sliceHeight / imgHeight) * canvas.height;

                        // 创建临时 canvas 来裁剪
                        const tempCanvas = document.createElement("canvas");
                        tempCanvas.width = canvas.width;
                        tempCanvas.height = Math.ceil(sourceHeight);
                        const ctx = tempCanvas.getContext("2d");
                        if (ctx) {
                            ctx.drawImage(
                                canvas,
                                0, sourceY, canvas.width, sourceHeight,
                                0, 0, canvas.width, sourceHeight
                            );
                            pdf.addImage(
                                tempCanvas.toDataURL("image/jpeg", 0.95),
                                "JPEG",
                                margin,
                                currentY,
                                imgWidth,
                                sliceHeight,
                            );
                        }

                        currentY += sliceHeight;
                        sourceY += sourceHeight;
                        remainingImgHeight -= sliceHeight;

                        // 如果还有剩余内容，换页继续
                        if (remainingImgHeight > 0) {
                            pdf.addPage();
                            currentY = margin;
                        }
                    }
                }
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

    /**
     * 导出为 Word (.docx) — 通过 Rust 后端转换
     */
    async exportToWord(
        data: ExportData,
        options: ConversationExportOptions,
        filename: string,
    ): Promise<void> {
        try {
            // 复用已有的 Markdown 格式化逻辑
            const markdown = formatAsMarkdown(data, options);

            // 调用 Rust 后端进行 markdown → docx 转换
            const docxBytes: number[] = await invoke("markdown_to_docx", { markdown });
            const content = new Uint8Array(docxBytes);
            const sanitizedName = sanitizeFilename(filename);

            await this.saveFile(
                content,
                `${sanitizedName}.docx`,
                [{ name: "Word", extensions: ["docx"] }],
            );

            toast.success("Word 导出成功");
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : "导出失败";
            if (errorMessage.includes("取消")) {
                return;
            }
            toast.error(`Word 导出失败: ${errorMessage}`);
            throw error;
        }
    },
};
