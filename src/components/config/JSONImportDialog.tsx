import React, { useState, useEffect, useCallback } from 'react';
import { Button } from '../ui/button';
import { Textarea } from '../ui/textarea';
import { Label } from '../ui/label';
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    CompactDialogFooter,
} from '../ui/dialog';
import { Input } from '../ui/input';
import { toast } from 'sonner';
import { MCPServerRequest } from '../../data/MCP';
import { readText } from '@tauri-apps/plugin-clipboard-manager';

interface JSONImportDialogProps {
    isOpen: boolean;
    onClose: () => void;
    onImport: (configs: MCPServerRequest[]) => void | Promise<void>;
    onCatalog?: (id: string) => Promise<void>;
}

// The stdio field is editable text. Preserve each imported argv entry, including
// empty strings, quotes and Windows paths, for the backend's literal argument parser.
function quoteStdioArgument(value: string): string {
    if (value && !/[\s'"\\]/.test(value)) return value;
    const trailing = value.match(/\\+$/)?.[0] ?? '';
    const body = value.slice(0, value.length - trailing.length).replaceAll("'", "\\'");
    // A trailing backslash before a single quote escapes that quote in legacy
    // commands. Encode the trailing run in its own double-quoted section.
    return "'" + body + "'" + (trailing ? '"' + trailing.repeat(2) + '"' : '');
}

const JSONImportDialog: React.FC<JSONImportDialogProps> = ({
    isOpen,
    onClose,
    onImport, onCatalog
}) => {
    const [jsonText, setJsonText] = useState('');
    const [preview, setPreview] = useState<MCPServerRequest[]>([]);
    const [catalog, setCatalog] = useState('');
    const [submitting, setSubmitting] = useState(false);
    const [isLoading, setIsLoading] = useState(false);

    // 如果剪贴板里的是 json 就直接加载到对话框
    useEffect(() => {
        if (isOpen) {
            loadClipboardContent();
        } else {
            setJsonText(''); setPreview([]); setCatalog('');
        }
    }, [isOpen]);

    const loadClipboardContent = useCallback(async () => {
        setIsLoading(true);
        try {
            const clipboardText = await readText();
            if (clipboardText && isValidJSON(clipboardText)) {
                setJsonText(clipboardText);
                toast.success('已自动从剪切板加载JSON内容');
            }
        } catch (error) {
            console.warn('Failed to read clipboard:', error);
        } finally {
            setIsLoading(false);
        }
    }, []);

    const isValidJSON = (text: string): boolean => {
        try {
            JSON.parse(text);
            return true;
        } catch {
            return false;
        }
    };

    const validateMCPConfig = (config: any): MCPServerRequest[] => {
        // 检查是否是标准MCP格式，包含mcpServers对象
        if (config.mcpServers && typeof config.mcpServers === 'object') {
            return parseMCPServersFormat(config.mcpServers);
        }

        // 检查是否是单个服务器配置（旧格式）
        if (config.name || config.transport_type || config.command || config.url) {
            return [parseSingleServerConfig(config)];
        }

        throw new Error('无效的JSON格式，请使用标准MCP配置格式或单个服务器配置格式');
    };

    const parseMCPServersFormat = (mcpServers: any): MCPServerRequest[] => {
        const servers: MCPServerRequest[] = [];

        for (const [serverName, serverConfig] of Object.entries(mcpServers)) {
            if (!serverConfig || typeof serverConfig !== 'object') {
                throw new Error(`服务器 "${serverName}" 配置无效`);
            }

            const config = serverConfig as any;
            let transport_type: string;
            let command = '';
            let url = '';
            let environment_variables = '';
            let headersJson: string | undefined = undefined;

            // 根据配置确定传输类型
            if (config.url) {
                if (!config.url.startsWith('http')) {
                    throw new Error(`服务器 "${serverName}" 的url必须是http/https`);
                }
                transport_type = 'http';
                url = config.url;
            } else if (config.command || config.args) {
                transport_type = 'stdio';
                if (typeof config.command !== 'string' || !config.command) {
                    throw new Error(`服务器 "${serverName}" 的stdio配置必须包含command字段`);
                }
                const args: unknown = config.args ?? [];
                if (!Array.isArray(args) || !args.every(arg => typeof arg === 'string')) {
                    throw new Error(`服务器 "${serverName}" 的 args 必须是字符串数组`);
                }
                command = [config.command, ...args].map(quoteStdioArgument).join(' ');
            } else {
                throw new Error(`服务器 "${serverName}" 必须包含url或command/args字段`);
            }

            // 处理环境变量
            if (config.env && typeof config.env === 'object') {
                environment_variables = Object.entries(config.env)
                    .map(([key, value]) => `${key}=${value}`)
                    .join('\n');
            }

            // headers (e.g. { "Authorization": "Bearer ${API_KEY}" })
            if (config.headers && typeof config.headers === 'object') {
                try {
                    headersJson = JSON.stringify(config.headers);
                } catch (e) {
                    console.warn('Invalid headers object for server', serverName, e);
                }
            }

            servers.push({
                name: serverName,
                description: config.description || `${serverName} MCP Server`,
                transport_type,
                command,
                environment_variables,
                url,
                headers: headersJson,
                timeout: config.timeout || 30000,
                is_long_running: config.is_long_running || false,
                is_enabled: false,
                proxy_enabled: false,
            });
        }

        return servers;
    };

    const parseSingleServerConfig = (config: any): MCPServerRequest => {
        // 必需字段
        if (!config.name || typeof config.name !== 'string') {
            throw new Error('缺少必需的字段: name');
        }

        if (!config.transport_type || !['stdio', 'http'].includes(config.transport_type)) {
            throw new Error('transport_type必须是stdio或http之一');
        }

        // 类型特定验证
        if (config.transport_type === 'stdio' && !config.command) {
            throw new Error('stdio类型必须提供command字段');
        }

        if ((config.transport_type === 'http') && !config.url) {
            throw new Error(`${config.transport_type}类型必须提供url字段`);
        }

        // 如果环境变量是对象，则转换为字符串
        let envVars = config.environment_variables || '';
        if (typeof envVars === 'object' && envVars !== null) {
            envVars = Object.entries(envVars)
                .map(([key, value]) => `${key}=${value}`)
                .join('\n');
        }

        return {
            name: config.name,
            description: config.description || '',
            transport_type: config.transport_type,
            command: config.command || '',
            environment_variables: envVars,
            headers: typeof config.headers === 'string' ? config.headers : config.headers ? JSON.stringify(config.headers) : undefined,
            url: config.url || '',
            timeout: config.timeout || 30000,
            is_long_running: config.is_long_running || false,
            is_enabled: false,
            proxy_enabled: false,
        };
    };

    const handleImport = async () => {
        if (submitting) return;
        setSubmitting(true);
        try {
            if (catalog) {
                if (!onCatalog) throw new Error('请在「应用与工具 → 从 JSON 导入」打开目录配置');
                await onCatalog(catalog);
            } else if (preview.length) {
                await onImport(preview);
            } else {
                const raw = JSON.parse(jsonText);
                if (raw?.format === 'aipp.connection.v1') {
                    if (typeof raw.app === 'string' && raw.app.trim()) setCatalog(raw.app);
                    else if (['http', 'stdio'].includes(raw.transport)) setPreview([{
                        name: '导入的 MCP', transport_type: raw.transport, url: typeof raw.url === 'string' ? raw.url : '', command: '',
                        timeout: Number.isInteger(raw.timeout) && raw.timeout > 0 ? raw.timeout : 30000,
                        is_long_running: raw.longRunning === true, proxy_enabled: false, is_enabled: false,
                    }]);
                    else throw new Error('无效的连接模板');
                } else {
                    const configs = validateMCPConfig(raw);
                    if (!configs.length) throw new Error('配置中没有连接');
                    setPreview(configs);
                }
            }
        } catch (error) { toast.error(String(error)); }
        finally { setSubmitting(false); }
    };
    const reviewing = !!catalog || preview.length > 0;
    const update = (i: number, key: 'name'|'command'|'url', value: string) =>
        setPreview(items => items.map((item, index) => index === i ? {...item, [key]: value} : item));
    const handleCancel = useCallback(() => {
        onClose();
    }, [onClose]);

    return (
        <Dialog open={isOpen} onOpenChange={(open) => !open && !submitting && handleCancel()}>
            <CompactDialogContent className="sm:max-w-2xl">
                <CompactDialogHeader
                    title="JSON 导入 MCP 服务器配置"
                    subtitle={reviewing ? '确认配置' : '粘贴 JSON'}
                    description="粘贴标准 MCP JSON 配置并预览导入"
                    closeDisabled={submitting}
                />

                <CompactDialogBody className="space-y-4">
                    {reviewing ? (
                        <div className="space-y-4">
                            {catalog && (
                                <p className="rounded-lg bg-muted/50 px-3 py-2 text-sm">
                                    来源：AIPP 应用目录 · {catalog}。打开详情可查看服务地址和登录要求，再决定是否连接。
                                </p>
                            )}
                            {preview.map((config, i) => (
                                <section key={i} className="space-y-3 rounded-lg border border-border p-3 text-sm">
                                    <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
                                        <span>来源：导入的自定义 MCP</span>
                                        <span className="rounded bg-muted px-1.5 py-0.5 font-mono">{config.transport_type}</span>
                                    </div>
                                    <label className="block space-y-1.5">
                                        <span className="text-sm font-medium">连接名称</span>
                                        <Input value={config.name} onChange={e => update(i, 'name', e.target.value)} />
                                    </label>
                                    <label className="block space-y-1.5">
                                        <span className="text-sm font-medium">{config.transport_type === 'stdio' ? '本地命令（启用后可执行）' : '服务地址'}</span>
                                        <Textarea
                                            rows={2}
                                            className="font-mono text-xs"
                                            value={config.transport_type === 'stdio' ? config.command ?? '' : config.url ?? ''}
                                            onChange={e => update(i, config.transport_type === 'stdio' ? 'command' : 'url', e.target.value)}
                                        />
                                    </label>
                                    <p className="text-xs text-muted-foreground">
                                        认证：{config.headers || config.environment_variables ? '配置含请求头或环境变量，请核对来源' : '未导入凭据，如服务需要请在设置中填写'}。
                                    </p>
                                </section>
                            ))}
                            {preview.length > 0 && (
                                <p className="text-xs text-muted-foreground">导入后保持停用。保存配置不会启动程序、登录或授权工具。</p>
                            )}
                        </div>
                    ) : (
                        <div className="space-y-3">
                            <div className="flex items-center justify-between gap-2">
                                <Label htmlFor="json-content">JSON配置</Label>
                                <div className="flex items-center gap-1.5">
                                    <Button
                                        type="button"
                                        variant="ghost"
                                        size="sm"
                                        className="h-7 px-2 text-xs"
                                        onClick={loadClipboardContent}
                                        disabled={isLoading}
                                    >
                                        {isLoading ? '读取中...' : '从剪切板读取'}
                                    </Button>
                                    <Button
                                        type="button"
                                        variant="ghost"
                                        size="sm"
                                        className="h-7 px-2 text-xs"
                                        onClick={() => setJsonText('')}
                                        disabled={!jsonText}
                                    >
                                        清空
                                    </Button>
                                </div>
                            </div>
                            <Textarea
                                id="json-content"
                                placeholder={`标准MCP配置格式：
{
  "mcpServers": {
    "context7": {
      "url": "https://mcp.context7.com/mcp"
    },
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/directory"]
    }
  }
}

或单个服务器配置：
{
  "name": "my-server",
  "transport_type": "stdio",
  "command": "npx server-command",
  "description": "My MCP server"
}`}
                                rows={14}
                                value={jsonText}
                                onChange={(e) => setJsonText(e.target.value)}
                                className="font-mono text-xs"
                            />
                        </div>
                    )}
                </CompactDialogBody>

                <CompactDialogFooter>
                    {reviewing && (
                        <Button
                            type="button"
                            variant="ghost"
                            disabled={submitting}
                            className="mr-auto min-h-11 px-3 sm:min-h-9"
                            onClick={() => { setCatalog(''); setPreview([]); }}
                        >
                            返回 JSON
                        </Button>
                    )}
                    <Button
                        type="button"
                        variant="outline"
                        onClick={handleCancel}
                        disabled={submitting}
                        className="min-h-11 px-4 sm:min-h-9"
                    >
                        取消
                    </Button>
                    <Button
                        type="button"
                        onClick={handleImport}
                        disabled={submitting || !jsonText.trim()}
                        className="min-h-11 px-5 sm:min-h-9"
                    >
                        {catalog ? '查看目录详情' : reviewing ? '保存为停用连接' : '预览配置'}
                    </Button>
                </CompactDialogFooter>
            </CompactDialogContent>
        </Dialog>
    );
};

export default JSONImportDialog;
