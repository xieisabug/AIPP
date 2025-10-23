import React, { useState, useEffect, useCallback } from 'react';
import { invoke } from "@tauri-apps/api/core";
import { Button } from '../ui/button';
import { Switch } from '../ui/switch';
import { Textarea } from '../ui/textarea';
import { Input } from '../ui/input';
import { Label } from '../ui/label';
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '../ui/dialog';
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '../ui/accordion';
import { toast } from 'sonner';
import { MCPServer, MCPServerRequest, MCP_TRANSPORT_TYPES } from '../../data/MCP';
import CustomSelect from '../CustomSelect';

interface MCPServerDialogProps {
    isOpen: boolean;
    onClose: () => void;
    onSubmit: () => void;
    editingServer?: MCPServer | null;
    initialServerType?: string;
    initialConfig?: Partial<MCPServerRequest>;
}

const MCPServerDialog: React.FC<MCPServerDialogProps> = ({
    isOpen,
    onClose,
    onSubmit,
    editingServer,
    initialServerType,
    initialConfig
}) => {
    // Form state
    const [formData, setFormData] = useState<MCPServerRequest>({
        name: '',
        description: '',
        transport_type: 'stdio',
        command: '',
        environment_variables: '',
        headers: '',
        url: '',
        timeout: 30000,
        is_long_running: false,
        is_enabled: true,
    });

    // UI state
    const [isSubmitting, setIsSubmitting] = useState(false);
    // Header rows state for SSE/HTTP
    type HeaderRow = { id: string; key: string; value: string };
    const [headerRows, setHeaderRows] = useState<HeaderRow[]>([]);

    // helpers for headers KV <-> JSON string
    const parseHeadersToRows = useCallback((headersStr?: string | null): HeaderRow[] => {
        if (!headersStr) return [];
        try {
            const v = JSON.parse(headersStr);
            if (v && typeof v === 'object' && !Array.isArray(v)) {
                const entries = Object.entries(v as Record<string, unknown>)
                    .filter(([, val]) => typeof val === 'string') as Array<[string, string]>;
                return entries.map(([k, v], idx) => ({ id: `${Date.now()}-${idx}-${k}`, key: k, value: v }));
            }
        } catch (_) {
            // ignore invalid JSON; keep empty rows
        }
        return [];
    }, []);

    const rowsToHeadersString = useCallback((rows: HeaderRow[]) => {
        const obj: Record<string, string> = {};
        rows.forEach(r => {
            const key = r.key.trim();
            if (key) obj[key] = r.value;
        });
        return Object.keys(obj).length ? JSON.stringify(obj, null, 2) : '';
    }, []);

    // 初始化界面数据
    useEffect(() => {
        if (editingServer) {
            // 尝试对已保存的 headers 进行 JSON pretty-print（仅当是合法 JSON 对象时）
            let prettyHeaders = editingServer.headers || '';
            if (prettyHeaders) {
                try {
                    const parsed = JSON.parse(prettyHeaders);
                    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
                        prettyHeaders = JSON.stringify(parsed, null, 2);
                    }
                } catch (_) {
                    // 保留原始字符串（用户可能保存了暂时不合法的内容，我们不强制格式化）
                }
            }
            setFormData({
                name: editingServer.name,
                description: editingServer.description || '',
                transport_type: editingServer.transport_type,
                command: editingServer.command || '',
                environment_variables: editingServer.environment_variables || '',
                headers: prettyHeaders,
                url: editingServer.url || '',
                timeout: editingServer.timeout || 30000,
                is_long_running: editingServer.is_long_running,
                is_enabled: editingServer.is_enabled,
            });
            // initialize header rows from existing headers
            setHeaderRows(parseHeadersToRows(prettyHeaders));
        } else {
            // 重置表单，使用可选的初始配置
            const defaultConfig: MCPServerRequest = {
                name: '',
                description: '',
                transport_type: initialServerType || 'stdio',
                command: '',
                environment_variables: '',
                headers: '',
                url: '',
                timeout: 30000,
                is_long_running: false,
                is_enabled: true,
            };

            // 合并初始配置
            const finalConfig = initialConfig ? { ...defaultConfig, ...initialConfig } : defaultConfig;

            setFormData(finalConfig);
            // initialize rows from default/initial config
            setHeaderRows(parseHeadersToRows(finalConfig.headers));
        }
    }, [editingServer, isOpen, initialServerType, initialConfig]);

    // 更新表单字段
    const updateField = useCallback((field: keyof MCPServerRequest, value: any) => {
        setFormData(prev => ({
            ...prev,
            [field]: value
        }));
    }, []);

    // Keep formData.headers in sync with headerRows for http/sse
    useEffect(() => {
        if (formData.transport_type === 'http' || formData.transport_type === 'sse') {
            const headersStr = rowsToHeadersString(headerRows);
            if (headersStr !== (formData.headers || '')) {
                setFormData(prev => ({ ...prev, headers: headersStr }));
            }
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [headerRows, formData.transport_type]);

    // When transport type changes away from http/sse, clear header rows sync (but keep stored string)
    useEffect(() => {
        if (formData.transport_type !== 'http' && formData.transport_type !== 'sse') {
            setHeaderRows([]);
        } else {
            // ensure rows reflect current headers string once upon switch
            setHeaderRows(parseHeadersToRows(formData.headers));
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [formData.transport_type]);

    const addHeaderRow = useCallback(() => {
        setHeaderRows(prev => [...prev, { id: `${Date.now()}-${prev.length}`, key: '', value: '' }]);
    }, []);
    const removeHeaderRow = useCallback((id: string) => {
        setHeaderRows(prev => prev.filter(r => r.id !== id));
    }, []);
    const updateHeaderRow = useCallback((id: string, patch: Partial<HeaderRow>) => {
        setHeaderRows(prev => prev.map(r => (r.id === id ? { ...r, ...patch } : r)));
    }, []);

    // 处理表单提交
    const handleSubmit = useCallback(async () => {
        // Validation
        if (!formData.name.trim()) {
            toast.error('请输入MCP服务器名称');
            return;
        }

        if (formData.transport_type === 'stdio' && !formData.command?.trim()) {
            toast.error('Stdio类型需要提供命令');
            return;
        }

        if ((formData.transport_type === 'sse' || formData.transport_type === 'http') && !formData.url?.trim()) {
            toast.error('SSE/HTTP类型需要提供URL');
            return;
        }

        setIsSubmitting(true);

        try {
            let serverId: number;

            if (editingServer) {
                await invoke('update_mcp_server', {
                    id: editingServer.id,
                    request: formData
                });
                serverId = editingServer.id;
                toast.success('更新MCP服务器成功');
            } else {
                serverId = await invoke<number>('add_mcp_server', {
                    request: formData
                });
                toast.success('添加MCP服务器成功');

                // 新增服务器后自动获取能力
                try {
                    await invoke('refresh_mcp_server_capabilities', { serverId });
                    toast.success('自动获取服务器能力成功');
                } catch (e) {
                    console.warn('自动获取能力失败:', e);
                    toast.warning('服务器添加成功，但获取能力失败，请手动刷新');
                }
            }

            onSubmit();
        } catch (e) {
            toast.error(`${editingServer ? '更新' : '添加'}MCP服务器失败: ${e}`);
        } finally {
            setIsSubmitting(false);
        }
    }, [formData, editingServer, onSubmit]);

    // 处理取消
    const handleCancel = useCallback(() => {
        onClose();
    }, [onClose]);

    return (
        <Dialog open={isOpen} onOpenChange={(open) => !open && handleCancel()}>
            <DialogContent className="max-w-3xl h-[85vh] flex flex-col">
                <DialogHeader>
                    <DialogTitle>
                        {editingServer ? '编辑MCP服务器' : '新增MCP服务器'}
                    </DialogTitle>
                </DialogHeader>
                <div className="space-y-6 py-4 overflow-y-auto pr-1 flex-1 min-h-0">
                    {/* 基本信息 */}
                    <div className="space-y-4">
                        <div className="space-y-2">
                            <Label htmlFor="name">ID *</Label>
                            <Input
                                id="name"
                                placeholder="例如：fetch-mcp"
                                value={formData.name}
                                onChange={(e) => updateField('name', e.target.value)}
                            />
                        </div>

                        <div className="space-y-2">
                            <Label htmlFor="description">描述</Label>
                            <Textarea
                                id="description"
                                placeholder="MCP功能描述..."
                                rows={2}
                                value={formData.description}
                                onChange={(e) => updateField('description', e.target.value)}
                            />
                        </div>


                        <div className="space-y-2">
                            <label className="text-sm font-medium text-foreground">MCP类型 *</label>
                            <CustomSelect
                                options={MCP_TRANSPORT_TYPES}
                                value={formData.transport_type}
                                onChange={(value) => updateField('transport_type', value)}
                            />
                        </div>


                        {/* Stdio specific fields */}
                        {formData.transport_type === 'stdio' && (
                            <div className="space-y-2">
                                <Label htmlFor="command">命令 *</Label>
                                <Textarea
                                    id="command"
                                    placeholder="例如：npx @modelcontextprotocol/server-filesystem /path/to/directory"
                                    rows={3}
                                    value={formData.command}
                                    onChange={(e) => updateField('command', e.target.value)}
                                />
                            </div>
                        )}

                        {/* SSE/HTTP specific fields */}
                        {(formData.transport_type === 'sse' || formData.transport_type === 'http') && (
                            <div className="space-y-2">
                                <Label htmlFor="url">URL *</Label>
                                <Input
                                    id="url"
                                    type="url"
                                    placeholder="例如：http://localhost:3000/mcp"
                                    value={formData.url}
                                    onChange={(e) => updateField('url', e.target.value)}
                                />
                            </div>
                        )}

                        <div className="space-y-2">
                            <Label htmlFor="environment_variables">环境变量</Label>
                            <Textarea
                                id="environment_variables"
                                placeholder="KEY1=value1&#10;KEY2=value2"
                                rows={5}
                                className="font-mono text-xs resize-none h-40 overflow-auto leading-5"
                                value={formData.environment_variables}
                                onChange={(e) => updateField('environment_variables', e.target.value)}
                            />
                        </div>
                    </div>

                    {/* 高级设置 */}
                    <Accordion type="single" collapsible>
                        <AccordionItem value="advanced">
                            <AccordionTrigger>高级设置</AccordionTrigger>
                            <AccordionContent className="space-y-4">
                                <div className="space-y-2">
                                    <Label htmlFor="timeout">请求超时 (毫秒)</Label>
                                    <Input
                                        id="timeout"
                                        type="number"
                                        min="1000"
                                        max="300000"
                                        step="1000"
                                        value={formData.timeout || 30000}
                                        onChange={(e) => updateField('timeout', parseInt(e.target.value) || 30000)}
                                    />
                                </div>

                                <div className="flex items-center justify-between">
                                    <div>
                                        <Label>是否长期运行</Label>
                                        <p className="text-sm text-muted-foreground mt-1">长期运行的服务器会保持连接状态</p>
                                    </div>
                                    <Switch
                                        checked={formData.is_long_running}
                                        onCheckedChange={(checked) => updateField('is_long_running', checked)}
                                    />
                                </div>

                                {(formData.transport_type === 'http' || formData.transport_type === 'sse') && (
                                    <div className="space-y-2">
                                        <Label>自定义请求头</Label>
                                        <div className="space-y-2">
                                            {headerRows.map((row) => (
                                                <div key={row.id} className="flex gap-2 items-center">
                                                    <Input
                                                        placeholder="Header 名称，例如 Authorization"
                                                        value={row.key}
                                                        onChange={(e) => updateHeaderRow(row.id, { key: e.target.value })}
                                                        className="flex-[0.45]"
                                                    />
                                                    <Input
                                                        placeholder="Header 值，例如 Bearer ${API_KEY}"
                                                        value={row.value}
                                                        onChange={(e) => updateHeaderRow(row.id, { value: e.target.value })}
                                                        className="flex-1"
                                                    />
                                                    <Button
                                                        type="button"
                                                        variant="outline"
                                                        onClick={() => removeHeaderRow(row.id)}
                                                    >
                                                        删除
                                                    </Button>
                                                </div>
                                            ))}
                                            <div>
                                                <Button type="button" variant="secondary" onClick={addHeaderRow}>
                                                    添加Header
                                                </Button>
                                            </div>
                                        </div>
                                        <p className="text-xs text-muted-foreground">支持 ${'{'}VAR{'}'} 环境变量占位符；重复键以最后一次填写为准；空键将被忽略</p>
                                    </div>
                                )}
                            </AccordionContent>
                        </AccordionItem>
                    </Accordion>
                </div>
                <DialogFooter className="flex-shrink-0">
                    <Button
                        variant="outline"
                        onClick={handleCancel}
                        disabled={isSubmitting}
                    >
                        取消
                    </Button>
                    <Button
                        onClick={handleSubmit}
                        disabled={isSubmitting}
                    >
                        {isSubmitting ? '保存中...' : '确定'}
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
};

export default MCPServerDialog;