import React from 'react';
import { File, Search, FolderOpen, FileInput, FileQuestion } from 'lucide-react';
import { ContextItem } from './types';
import { cn } from '@/utils/utils';

interface ContextListProps {
    items: ContextItem[];
    className?: string;
    onItemClick?: (item: ContextItem) => void;
}

const getContextIcon = (type: ContextItem['type']) => {
    switch (type) {
        case 'user_file':
            return <FileInput className="h-4 w-4 text-blue-500 flex-shrink-0" />;
        case 'read_file':
            return <File className="h-4 w-4 text-green-500 flex-shrink-0" />;
        case 'search':
            return <Search className="h-4 w-4 text-purple-500 flex-shrink-0" />;
        case 'list_directory':
            return <FolderOpen className="h-4 w-4 text-yellow-500 flex-shrink-0" />;
        default:
            return <FileQuestion className="h-4 w-4 text-muted-foreground flex-shrink-0" />;
    }
};

const getContextLabel = (type: ContextItem['type']): string => {
    switch (type) {
        case 'user_file':
            return '用户文件';
        case 'read_file':
            return '读取文件';
        case 'search':
            return '搜索';
        case 'list_directory':
            return '目录';
        default:
            return '其他';
    }
};

const ContextList: React.FC<ContextListProps> = ({ items, className, onItemClick }) => {
    if (items.length === 0) {
        return (
            <div className={cn("p-3 text-sm text-muted-foreground text-center", className)}>
                暂无上下文
            </div>
        );
    }

    // Group by type
    const groupedItems = items.reduce((acc, item) => {
        if (!acc[item.type]) {
            acc[item.type] = [];
        }
        acc[item.type].push(item);
        return acc;
    }, {} as Record<ContextItem['type'], ContextItem[]>);

    return (
        <div className={cn("flex flex-col gap-1 p-2 max-h-48 overflow-y-auto", className)}>
            {Object.entries(groupedItems).map(([type, typeItems]) => (
                <div key={type} className="flex flex-col gap-1">
                    <div className="text-xs font-medium text-muted-foreground px-2 pt-1">
                        {getContextLabel(type as ContextItem['type'])} ({typeItems.length})
                    </div>
                    {typeItems.map((item) => (
                        <div
                            key={item.id}
                            className={cn(
                                "flex items-center gap-2 p-2 rounded-md transition-colors",
                                "hover:bg-muted/50",
                                onItemClick && "cursor-pointer"
                            )}
                            onClick={() => onItemClick?.(item)}
                        >
                            {getContextIcon(item.type)}
                            <div className="flex-1 min-w-0">
                                <p className="text-sm truncate">{item.name}</p>
                                {item.details && item.details !== item.name && (
                                    <p className="text-xs text-muted-foreground truncate">{item.details}</p>
                                )}
                            </div>
                        </div>
                    ))}
                </div>
            ))}
        </div>
    );
};

export default React.memo(ContextList);
