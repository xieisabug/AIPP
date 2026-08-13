import React, { useState, useEffect, memo } from 'react';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../ui/select';

export interface SelectOption {
    id: string;
    label: string;
    icon?: React.ReactNode;
}

interface ConfigPageLayoutProps {
    sidebar: React.ReactNode;
    content: React.ReactNode;
    emptyState?: React.ReactNode;
    showEmptyState?: boolean;
    // 响应式下拉菜单相关props
    selectOptions?: SelectOption[];
    selectedOptionId?: string;
    onSelectOption?: (optionId: string) => void;
    selectPlaceholder?: string;
    addButton?: React.ReactNode;
}

const ConfigPageLayout: React.FC<ConfigPageLayoutProps> = ({
    sidebar,
    content,
    emptyState,
    showEmptyState = false,
    selectOptions,
    selectedOptionId,
    onSelectOption,
    selectPlaceholder = "选择项目",
    addButton,
}) => {
    const [windowWidth, setWindowWidth] = useState(window.innerWidth);

    useEffect(() => {
        const handleResize = () => {
            setWindowWidth(window.innerWidth);
        };

        window.addEventListener('resize', handleResize);
        
        // 清理事件监听器
        return () => {
            window.removeEventListener('resize', handleResize);
        };
    }, []);

    // 小屏幕时使用下拉菜单（宽度小于1200px）
    const isSmallScreen = windowWidth < 1200;
    const shouldShowDropdown = isSmallScreen && selectOptions && selectOptions.length > 0;

    const renderDropdownHeader = () => {
        if (!shouldShowDropdown) return null;

        const selectedOption = selectOptions?.find(option => option.id === selectedOptionId);

        return (
            <div className="flex-shrink-0 px-4 pt-6 pb-4">
                <div className="flex items-center gap-3">
                    <div className="flex-1">
                        <Select value={selectedOptionId} onValueChange={onSelectOption}>
                            <SelectTrigger className="w-full">
                                <SelectValue placeholder={selectPlaceholder}>
                                    {selectedOption && (
                                        <div className="flex items-center gap-2">
                                            {selectedOption.icon}
                                            <span>{selectedOption.label}</span>
                                        </div>
                                    )}
                                </SelectValue>
                            </SelectTrigger>
                            <SelectContent>
                                {selectOptions?.map((option) => (
                                    <SelectItem key={option.id} value={option.id}>
                                        <div className="flex items-center gap-2">
                                            {option.icon}
                                            <span>{option.label}</span>
                                        </div>
                                    </SelectItem>
                                ))}
                            </SelectContent>
                        </Select>
                    </div>
                    {addButton && (
                        <div className="flex-shrink-0">
                            {addButton}
                        </div>
                    )}
                </div>
            </div>
        );
    };

    return (
        <div className="flex flex-col h-full min-h-0 max-w-none">
            {/* 响应式下拉菜单 - 小屏幕时显示 */}
            {renderDropdownHeader()}

            {/* 主要内容区域 */}
            {showEmptyState ? (
                <div className="flex-1 min-h-0 overflow-y-auto thin-scrollbar px-4 py-6">
                    {emptyState}
                </div>
            ) : (
                <div className={`flex-1 min-h-0 grid gap-6 px-4 py-6 ${shouldShowDropdown ? 'grid-cols-1' : 'grid-cols-12'}`}>
                    {/* 左侧列表 - 大屏幕时显示，列表内部独立滚动（标题/搜索栏固定） */}
                    {!shouldShowDropdown && (
                        <div className="col-span-12 lg:col-span-4 xl:col-span-4 2xl:col-span-3 min-h-0">
                            {sidebar}
                        </div>
                    )}

                    {/* 右侧配置区域，独立滚动 */}
                    <div className={`min-h-0 overflow-y-auto thin-scrollbar ${shouldShowDropdown ? 'col-span-1' : 'col-span-12 lg:col-span-8 xl:col-span-8 2xl:col-span-9'}`}>
                        {content}
                    </div>
                </div>
            )}
        </div>
    );
};

export default memo(ConfigPageLayout); 