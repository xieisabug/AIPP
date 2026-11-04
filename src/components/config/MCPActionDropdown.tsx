import React from 'react';
import { Button } from '../ui/button';
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuLabel,
    DropdownMenuSeparator,
    DropdownMenuTrigger,
} from '../ui/dropdown-menu';
import { ChevronDown, Plus } from 'lucide-react';
import { MCP_QUICK_TEMPLATES, MCPTemplate } from '../../data/MCPTemplates';
import { useIsMobilePlatform } from '@/hooks/use-platform';
import { isAippBuiltinCommand, isMobileExecutableMcpServer } from '@/lib/mobileUnsupported';

interface MCPActionDropdownProps {
    onTemplateSelect: (template: MCPTemplate) => void;
    onJSONImport: () => void;
    className?: string;
    variant?: 'default' | 'outline' | 'secondary' | 'ghost' | 'link' | 'destructive';
    size?: 'default' | 'sm' | 'lg' | 'icon';
    showIcon?: boolean;
    disabled?: boolean;
}

const MCPActionDropdown: React.FC<MCPActionDropdownProps> = ({
    onTemplateSelect,
    onJSONImport,
    className = '',
    variant = 'default',
    size = 'default',
    showIcon = true,
    disabled = false
}) => {
    const isMobilePlatform = useIsMobilePlatform();
    const quickStartTemplates = MCP_QUICK_TEMPLATES.filter((t) => {
        if (t.category !== 'Quick Start') return false;
        if (!isMobilePlatform) return true;
        return isMobileExecutableMcpServer({
            transport_type: t.template.transport_type,
            command: t.template.command,
            is_builtin: t.template.is_builtin,
        });
    });
    const importTemplates = MCP_QUICK_TEMPLATES.filter((t) => {
        if (t.category !== 'Import') return false;
        if (!isMobilePlatform) return true;
        // 保留 JSON 导入与 aipp:* 内置；隐藏会引导外部 stdio 的项
        if (t.id === 'json-import') return true;
        return isAippBuiltinCommand(t.template.command) || Boolean(t.template.is_builtin);
    });

    const handleTemplateClick = (template: MCPTemplate) => {
        if (template.id === 'json-import') {
            onJSONImport();
        } else {
            onTemplateSelect(template);
        }
    };

    return (
        <DropdownMenu>
            <DropdownMenuTrigger asChild>
                <Button
                    variant={variant}
                    size={size}
                    className={`gap-2 ${className}`}
                    disabled={disabled}
                >
                    {showIcon && <Plus className="h-4 w-4" />}
                    <ChevronDown className="h-4 w-4" />
                </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-56">
                <DropdownMenuLabel>快速添加</DropdownMenuLabel>
                {quickStartTemplates.map((template) => (
                    <DropdownMenuItem
                        key={template.id}
                        onClick={() => handleTemplateClick(template)}
                        className="flex items-center gap-2 cursor-pointer"
                    >
                        <div className="flex flex-col">
                            <span className="font-medium">{template.name}</span>
                            <span className="text-xs text-gray-500">{template.description}</span>
                        </div>
                    </DropdownMenuItem>
                ))}
                
                <DropdownMenuSeparator />
                
                <DropdownMenuLabel>导入配置</DropdownMenuLabel>
                {importTemplates.map((template) => (
                    <DropdownMenuItem
                        key={template.id}
                        onClick={() => handleTemplateClick(template)}
                        className="flex items-center gap-2 cursor-pointer"
                    >
                        <div className="flex flex-col">
                            <span className="font-medium">{template.name}</span>
                            <span className="text-xs text-gray-500">{template.description}</span>
                        </div>
                    </DropdownMenuItem>
                ))}
            </DropdownMenuContent>
        </DropdownMenu>
    );
};

export default MCPActionDropdown;