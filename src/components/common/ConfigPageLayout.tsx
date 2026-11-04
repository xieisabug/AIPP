import React, { useState, useEffect, useMemo, memo } from 'react';
import { Search } from 'lucide-react';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../ui/select';
import { Input } from '../ui/input';
import { PinyinFilter, usePinyinReady } from '../../utils/pinyinFilter';

/** 窄屏下拉超过此数量时显示搜索框（替代宽屏侧栏里的搜索）。 */
const DROPDOWN_SEARCH_THRESHOLD = 5;

export interface SelectOption {
    id: string;
    label: string;
    description?: string;
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
    const showDropdownSearch = Boolean(shouldShowDropdown && selectOptions && selectOptions.length > DROPDOWN_SEARCH_THRESHOLD);
    const [dropdownQuery, setDropdownQuery] = useState('');
    const pinyinReady = usePinyinReady(showDropdownSearch && dropdownQuery.trim().length > 0);
    const filteredSelectOptions = useMemo(() => {
        const query = dropdownQuery.trim();
        if (!showDropdownSearch || !query) return selectOptions ?? [];
        return (selectOptions ?? []).filter((option) =>
            PinyinFilter.matches(option.label, query) || (option.description ? PinyinFilter.matches(option.description, query) : false)
        );
    }, [selectOptions, dropdownQuery, showDropdownSearch, pinyinReady]);

    const renderDropdownHeader = () => {
        if (!shouldShowDropdown) return null;

        const selectedOption = selectOptions?.find(option => option.id === selectedOptionId);

        return (
            <div className="flex-shrink-0 px-4 pt-6 pb-4">
                {showDropdownSearch && (
                    <div className="relative mb-3">
                        <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                        <Input
                            type="search"
                            aria-label="搜索列表"
                            placeholder="搜索..."
                            enterKeyHint="search"
                            className="min-h-11 pl-9"
                            value={dropdownQuery}
                            onChange={(event) => setDropdownQuery(event.target.value)}
                        />
                    </div>
                )}
                {showDropdownSearch && dropdownQuery.trim() && filteredSelectOptions.length === 0 && (
                    <p className="mb-3 text-xs text-muted-foreground">没有匹配的项目</p>
                )}
                <div className="flex items-center gap-3">
                    <div className="min-w-0 flex-1">
                        <Select value={selectedOptionId} onValueChange={onSelectOption}>
                            <SelectTrigger className="w-full h-auto min-h-11 [&>[data-slot=select-value]]:min-w-0 [&>[data-slot=select-value]]:line-clamp-none">
                                <SelectValue placeholder={selectPlaceholder}>
                                    {selectedOption && (
                                        <div className="flex min-w-0 items-center gap-2 text-left">
                                            {selectedOption.icon}
                                            <span className="min-w-0 flex-1">
                                                <span className="block truncate">{selectedOption.label}</span>
                                                {selectedOption.description && <span className="block text-xs text-muted-foreground">{selectedOption.description}</span>}
                                            </span>
                                        </div>
                                    )}
                                </SelectValue>
                            </SelectTrigger>
                            <SelectContent className="w-[var(--radix-select-trigger-width)]">
                                {filteredSelectOptions.map((option) => (
                                    <SelectItem key={option.id} value={option.id} className="min-h-11 [&>[data-slot=select-item-text]]:min-w-0 [&>span:last-child]:min-w-0 [&>span:last-child]:flex-1">
                                        <div className="flex min-w-0 flex-1 items-center gap-2">
                                            {option.icon}
                                            <span className="min-w-0 flex-1">
                                                <span className="block truncate">{option.label}</span>
                                                {option.description && <span className="block text-xs text-muted-foreground">{option.description}</span>}
                                            </span>
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
