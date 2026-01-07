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
import { ChevronDown, PlusCircle } from 'lucide-react';

interface SkillActionDropdownProps {
    onScan: () => void;
    onOpenFolder: () => void;
    onInstallOfficial?: () => void;
    className?: string;
    variant?: 'default' | 'outline' | 'secondary' | 'ghost' | 'link' | 'destructive';
    size?: 'default' | 'sm' | 'lg' | 'icon';
    showIcon?: boolean;
    disabled?: boolean;
    isScanning?: boolean;
}

const SkillActionDropdown: React.FC<SkillActionDropdownProps> = ({
    onScan,
    onOpenFolder,
    onInstallOfficial,
    className = '',
    variant = 'default',
    size = 'default',
    showIcon = true,
    disabled = false,
    isScanning = false
}) => {
    return (
        <DropdownMenu>
            <DropdownMenuTrigger asChild>
                <Button
                    variant={variant}
                    size={size}
                    className={`gap-2 ${className}`}
                    disabled={disabled || isScanning}
                >
                    {showIcon && <PlusCircle className="h-4 w-4" />}
                    <ChevronDown className="h-4 w-4" />
                </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-56">
                <DropdownMenuLabel>扫描发现</DropdownMenuLabel>
                <DropdownMenuItem
                    onClick={onScan}
                    className="flex items-center gap-2 cursor-pointer"
                    disabled={isScanning}
                >
                    <div className="flex flex-col">
                        <span className="font-medium">扫描Skills</span>
                        <span className="text-xs text-muted-foreground">扫描 Claude Code、Codex 等目录</span>
                    </div>
                </DropdownMenuItem>
                
                <DropdownMenuSeparator />
                
                <DropdownMenuLabel>安装管理</DropdownMenuLabel>
                {onInstallOfficial && (
                    <DropdownMenuItem
                        onClick={onInstallOfficial}
                        className="flex items-center gap-2 cursor-pointer"
                    >
                        <div className="flex flex-col">
                            <span className="font-medium">安装AIPP官方Skills</span>
                            <span className="text-xs text-muted-foreground">从官方仓库安装推荐技能</span>
                        </div>
                    </DropdownMenuItem>
                )}
                <DropdownMenuItem
                    onClick={onOpenFolder}
                    className="flex items-center gap-2 cursor-pointer"
                >
                    <div className="flex flex-col">
                        <span className="font-medium">打开Skills文件夹</span>
                        <span className="text-xs text-muted-foreground">手动安装或管理Skills</span>
                    </div>
                </DropdownMenuItem>
            </DropdownMenuContent>
        </DropdownMenu>
    );
};

export default SkillActionDropdown;
