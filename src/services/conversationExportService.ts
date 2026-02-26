import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { writeFile } from "@tauri-apps/plugin-fs";
import { openPath } from "@tauri-apps/plugin-opener";
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
} from "@/utils/exportFormatters";
import {
    renderExportContent,
} from "@/components/conversation/ConversationExportRenderer";

/**
 * 对话导出服务
 */
export const conversationExportService = {
    showExportSuccess(formatName: string, filePath: string): void {
        toast.success(`${formatName} 导出成功`, {
            position: "bottom-right",
            action: {
                label: "打开",
                onClick: () => {
                    void openPath(filePath).catch((error) => {
                        const errorMessage = error instanceof Error ? error.message : "打开失败";
                        toast.error(`打开文件失败: ${errorMessage}`);
                    });
                },
            },
        });
    },

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
            const exportMessages = conversation.messages;
            const messageIdSet = new Set(exportMessages.map((msg) => msg.id));
            const hintCallIdSet = new Set<number>();
            for (const message of exportMessages) {
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
                    messages: exportMessages,
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
    ): Promise<string | null> {
        try {
            const path = await save({
                defaultPath: defaultName,
                filters,
            });
            if (!path) {
                return null;
            }
            await writeFile(path, content);
            return path;
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
    ): Promise<boolean> {
        try {
            const markdown = formatAsMarkdown(data, options);
            const encoder = new TextEncoder();
            const content = encoder.encode(markdown);
            const sanitizedName = sanitizeFilename(filename);
            const savePath = await this.saveFile(
                content,
                `${sanitizedName}.md`,
                [{ name: "Markdown", extensions: ["md"] }],
            );
            if (!savePath) {
                return false;
            }
            this.showExportSuccess("Markdown", savePath);
            return true;
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : "导出失败";
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

    async waitForImages(container: HTMLElement): Promise<void> {
        const images = Array.from(container.querySelectorAll("img"));
        if (images.length === 0) {
            return;
        }
        await Promise.all(
            images.map((image) => {
                if (image.complete) {
                    return Promise.resolve();
                }
                return new Promise<void>((resolve) => {
                    const done = () => {
                        resolve();
                    };
                    image.addEventListener("load", done, { once: true });
                    image.addEventListener("error", done, { once: true });
                });
            }),
        );
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

            const contentHtml = await this.markdownToHtml(stripMcpToolCallMarkers(message.content || ""));
            msgContainer.innerHTML = `
                <div style="padding: 16px 0; border-bottom: 2px solid #e0e0e0;">
                    <div style="${labelStyle}">${this.escapeHtml(label)}</div>
                    <div style="color: #111; font-size: 12px; line-height: 1.6; margin-top: 8px;">${contentHtml}</div>
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

    // Markdown 转 HTML（代码块通过 Rust syntect 高亮）
    async markdownToHtml(md: string, isDark: boolean = false): Promise<string> {
        // 先提取代码块，用占位符替换，避免 escapeHtml 破坏代码
        const codeBlocks: { lang: string; code: string }[] = [];
        const placeholder = (i: number) => `__CODE_BLOCK_${i}__`;
        const withPlaceholders = md.replace(/```(\w*)\n([\s\S]*?)```/g, (_match, lang, code) => {
            const idx = codeBlocks.length;
            codeBlocks.push({ lang: lang || "", code });
            return placeholder(idx);
        });

        // 对非代码部分进行 HTML 转义和 markdown 处理
        let html = this.escapeHtml(withPlaceholders);

        // 批量调用 Rust highlight_code 获取高亮 HTML
        const highlightedBlocks = await Promise.all(
            codeBlocks.map(async ({ lang, code }) => {
                try {
                    return await invoke<string>("highlight_code", {
                        lang: lang || "text",
                        code,
                        isDark,
                        themeHint: null,
                    });
                } catch {
                    // fallback：无高亮的纯文本代码块
                    return `<pre style="background: #f5f5f5; padding: 10px; border-radius: 4px; margin: 8px 0; overflow-x: auto;"><code style="font-family: Consolas, Monaco, monospace; font-size: 12px; color: #333; white-space: pre-wrap; word-break: break-word;">${this.escapeHtml(code)}</code></pre>`;
                }
            }),
        );

        // 将高亮后的代码块替换回去，添加容器样式
        for (let i = 0; i < highlightedBlocks.length; i++) {
            // Rust syntect 返回的 HTML 已含 <pre> + inline style，直接使用
            const styledBlock = `<div style="margin: 8px 0; border-radius: 6px; overflow: hidden; font-size: 12px; line-height: 1.5;">${highlightedBlocks[i]}</div>`;
            html = html.replace(placeholder(i), styledBlock);
        }

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
    ): Promise<boolean> {
        try {
            const markdown = formatAsMarkdown(data, options, {
                includeImageAttachments: true,
            });
            const pdfBytes: number[] = await invoke("markdown_to_pdf", { markdown });

            // 保存 PDF
            const sanitizedName = sanitizeFilename(filename);
            const content = new Uint8Array(pdfBytes);

            const savePath = await this.saveFile(
                content,
                `${sanitizedName}.pdf`,
                [{ name: "PDF", extensions: ["pdf"] }],
            );
            if (!savePath) {
                return false;
            }
            this.showExportSuccess("PDF", savePath);
            return true;
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : "导出失败";
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
    ): Promise<boolean> {
        try {
            // 渲染为 canvas
            const canvas = await this.renderToCanvas(data, options);
            const blob = await new Promise<Blob>((resolve, reject) => {
                canvas.toBlob((currentBlob) => {
                    if (!currentBlob) {
                        reject(new Error("无法生成图片"));
                        return;
                    }
                    resolve(currentBlob);
                }, "image/png");
            });
            const arrayBuffer = await blob.arrayBuffer();
            const content = new Uint8Array(arrayBuffer);
            const sanitizedName = sanitizeFilename(filename);
            const savePath = await this.saveFile(
                content,
                `${sanitizedName}.png`,
                [{ name: "PNG", extensions: ["png"] }],
            );
            if (!savePath) {
                return false;
            }
            this.showExportSuccess("PNG", savePath);
            return true;
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : "导出失败";
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
    ): Promise<boolean> {
        try {
            // 复用已有的 Markdown 格式化逻辑
            const markdown = formatAsMarkdown(data, options, {
                includeImageAttachments: true,
            });

            // 调用 Rust 后端进行 markdown → docx 转换
            const docxBytes: number[] = await invoke("markdown_to_docx", { markdown });
            const content = new Uint8Array(docxBytes);
            const sanitizedName = sanitizeFilename(filename);

            const savePath = await this.saveFile(
                content,
                `${sanitizedName}.docx`,
                [{ name: "Word", extensions: ["docx"] }],
            );
            if (!savePath) {
                return false;
            }
            this.showExportSuccess("Word", savePath);
            return true;
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : "导出失败";
            toast.error(`Word 导出失败: ${errorMessage}`);
            throw error;
        }
    },

    // ---- 单条消息导出 ----

    /**
     * 生成单条消息的默认文件名
     */
    singleMessageFilename(messageType: string): string {
        const label = this.getMessageLabel(messageType).replace(/[^\w\u4e00-\u9fff]/g, "");
        const now = new Date();
        const ts = `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, "0")}${String(now.getDate()).padStart(2, "0")}_${String(now.getHours()).padStart(2, "0")}${String(now.getMinutes()).padStart(2, "0")}`;
        return sanitizeFilename(`${label}_${ts}`);
    },

    /**
     * 导出单条消息为 Markdown
     */
    async exportSingleMessageToMarkdown(
        content: string,
        messageType: string,
    ): Promise<boolean> {
        try {
            const markdown = stripMcpToolCallMarkers(content);
            const encoded = new TextEncoder().encode(markdown);
            const filename = this.singleMessageFilename(messageType);
            const savePath = await this.saveFile(encoded, `${filename}.md`, [
                { name: "Markdown", extensions: ["md"] },
            ]);
            if (!savePath) return false;
            this.showExportSuccess("Markdown", savePath);
            return true;
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : "导出失败";
            toast.error(`Markdown 导出失败: ${errorMessage}`);
            throw error;
        }
    },

    /**
     * 导出单条消息为 Word
     */
    async exportSingleMessageToWord(
        content: string,
        messageType: string,
    ): Promise<boolean> {
        try {
            const markdown = stripMcpToolCallMarkers(content);
            const docxBytes: number[] = await invoke("markdown_to_docx", { markdown });
            const encoded = new Uint8Array(docxBytes);
            const filename = this.singleMessageFilename(messageType);
            const savePath = await this.saveFile(encoded, `${filename}.docx`, [
                { name: "Word", extensions: ["docx"] },
            ]);
            if (!savePath) return false;
            this.showExportSuccess("Word", savePath);
            return true;
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : "导出失败";
            toast.error(`Word 导出失败: ${errorMessage}`);
            throw error;
        }
    },

    /**
     * 渲染单条消息到 canvas
     */
    async renderSingleMessageToCanvas(
        content: string,
        messageType: string,
        width: number = 800,
    ): Promise<HTMLCanvasElement> {
        const { default: html2canvas } = await import("html2canvas");
        const isDarkMode = document.documentElement.classList.contains("dark");
        const backgroundColor = isDarkMode ? "#0a0a0b" : "#ffffff";

        const container = document.createElement("div");
        container.style.position = "fixed";
        container.style.left = "-9999px";
        container.style.top = "0";
        container.style.width = `${width}px`;
        container.style.background = backgroundColor;
        container.style.padding = "24px";
        container.style.fontFamily =
            '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, "Microsoft YaHei", sans-serif';
        container.style.color = isDarkMode ? "#fafafa" : "#0a0a0b";
        document.body.appendChild(container);

        try {
            const label = this.getMessageLabel(messageType);
            const labelStyle = this.getMessageLabelStyle(messageType);
            const sanitized = stripMcpToolCallMarkers(content);
            const contentHtml = await this.markdownToHtml(sanitized, isDarkMode);
            container.innerHTML = `
                <div style="padding: 16px 0;">
                    <div style="${labelStyle}">${this.escapeHtml(label)}</div>
                    <div style="color: inherit; font-size: 14px; line-height: 1.7; margin-top: 12px;">${contentHtml}</div>
                </div>
            `;

            await new Promise((resolve) => setTimeout(resolve, 200));
            return await html2canvas(container, {
                scale: 2,
                useCORS: true,
                logging: false,
                backgroundColor,
            });
        } finally {
            if (container.parentNode) {
                container.parentNode.removeChild(container);
            }
        }
    },

    /**
     * 导出单条消息为 PDF
     */
    async exportSingleMessageToPDF(
        content: string,
        messageType: string,
    ): Promise<boolean> {
        try {
            const { jsPDF } = await import("jspdf");
            const canvas = await this.renderSingleMessageToCanvas(content, messageType, 595);
            const pdf = new jsPDF({ orientation: "portrait", unit: "pt", format: "a4" });
            const pageWidth = pdf.internal.pageSize.getWidth();
            const margin = 20;
            const imgWidth = pageWidth - margin * 2;
            const imgHeight = (canvas.height * imgWidth) / canvas.width;
            pdf.addImage(
                canvas.toDataURL("image/jpeg", 0.95),
                "JPEG",
                margin,
                margin,
                imgWidth,
                imgHeight,
            );
            const filename = this.singleMessageFilename(messageType);
            const pdfBytes = pdf.output("arraybuffer");
            const encoded = new Uint8Array(pdfBytes);
            const savePath = await this.saveFile(encoded, `${filename}.pdf`, [
                { name: "PDF", extensions: ["pdf"] },
            ]);
            if (!savePath) return false;
            this.showExportSuccess("PDF", savePath);
            return true;
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : "导出失败";
            toast.error(`PDF 导出失败: ${errorMessage}`);
            throw error;
        }
    },

    /**
     * 导出单条消息为 PNG
     */
    async exportSingleMessageToPNG(
        content: string,
        messageType: string,
    ): Promise<boolean> {
        try {
            const canvas = await this.renderSingleMessageToCanvas(content, messageType);
            const blob = await new Promise<Blob>((resolve, reject) => {
                canvas.toBlob((b) => (b ? resolve(b) : reject(new Error("无法生成图片"))), "image/png");
            });
            const arrayBuffer = await blob.arrayBuffer();
            const encoded = new Uint8Array(arrayBuffer);
            const filename = this.singleMessageFilename(messageType);
            const savePath = await this.saveFile(encoded, `${filename}.png`, [
                { name: "PNG", extensions: ["png"] },
            ]);
            if (!savePath) return false;
            this.showExportSuccess("PNG", savePath);
            return true;
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : "导出失败";
            toast.error(`PNG 导出失败: ${errorMessage}`);
            throw error;
        }
    },
};
