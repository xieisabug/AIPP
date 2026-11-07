import React, { useState, useEffect, useMemo, memo } from 'react';
import { ChevronLeft, ChevronRight, Search } from 'lucide-react';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../ui/select';
import { Input } from '../ui/input';
import { PinyinFilter, usePinyinReady } from '../../utils/pinyinFilter';
import { useIsMobile } from '../../hooks/use-mobile';
import { useIsMobilePlatform } from '../../hooks/use-platform';
import { requestConfigLeave } from '@/lib/configLeaveGuard';
import { setConfigSectionDetail, useConfigSectionId } from '@/lib/configSectionNav';

/** 窄屏下拉超过此数量时显示搜索框（替代宽屏侧栏里的搜索）。 */
const DROPDOWN_SEARCH_THRESHOLD = 5;

export interface SelectOption {
    id: string;
    label: string;
    description?: string;
    icon?: React.ReactNode;
    /** 移动端列表行右侧的独立操作（如启用开关），点击不会进入详情。 */
    trailing?: React.ReactNode;
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
    /** 移动端列表是否提供搜索（固定的少量条目可关闭）。 */
    listSearchable?: boolean;
    /** 递增即在移动端打开当前选中项的详情（用于跨页跳转到某一项）。 */
    mobileDetailRequest?: number;
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
    listSearchable = true,
    mobileDetailRequest,
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

    // 移动端（移动平台或 <768px）：列表 → 详情两级导航，替代下拉切换
    const isMobilePlatform = useIsMobilePlatform();
    const isNarrow = useIsMobile();
    const useMasterDetail = (isMobilePlatform || isNarrow) && Boolean(selectOptions && selectOptions.length > 0);
    // 小屏幕时使用下拉菜单（宽度小于1200px）
    const isSmallScreen = windowWidth < 1200;
    const shouldShowDropdown = !useMasterDetail && isSmallScreen && selectOptions && selectOptions.length > 0;
    const showDropdownSearch = Boolean(
        (shouldShowDropdown || (useMasterDetail && listSearchable))
        && selectOptions && selectOptions.length > DROPDOWN_SEARCH_THRESHOLD
    );
    const [dropdownQuery, setDropdownQuery] = useState('');
    const pinyinReady = usePinyinReady(showDropdownSearch && dropdownQuery.trim().length > 0);
    const filteredSelectOptions = useMemo(() => {
        const query = dropdownQuery.trim();
        if (!showDropdownSearch || !query) return selectOptions ?? [];
        return (selectOptions ?? []).filter((option) =>
            PinyinFilter.matches(option.label, query) || (option.description ? PinyinFilter.matches(option.description, query) : false)
        );
    }, [selectOptions, dropdownQuery, showDropdownSearch, pinyinReady]);

    const [mobileView, setMobileView] = useState<'list' | 'detail'>('list');
    const showDetail = useMasterDetail && mobileView === 'detail' && !showEmptyState;
    const selectedLabel = selectOptions?.find(option => option.id === selectedOptionId)?.label ?? null;
    const sectionId = useConfigSectionId();
    useEffect(() => {
        if (mobileDetailRequest) setMobileView('detail');
    }, [mobileDetailRequest]);
    // 选中项被删除等情况：回到列表，避免停在空详情
    useEffect(() => {
        if (useMasterDetail && mobileView === 'detail' && selectedLabel === null) setMobileView('list');
    }, [useMasterDetail, mobileView, selectedLabel]);
    // 发布当前详情：设置顶栏显示详情标题，顶栏/系统返回先回到列表
    useEffect(() => {
        if (!sectionId) return;
        setConfigSectionDetail(sectionId, showDetail
            ? { title: selectedLabel ?? '', back: () => requestConfigLeave(() => setMobileView('list')) }
            : null);
    }, [sectionId, showDetail, selectedLabel]);
    useEffect(() => () => { if (sectionId) setConfigSectionDetail(sectionId, null); }, [sectionId]);

    const openDetail = (optionId: string) => {
        onSelectOption?.(optionId);
        setMobileView('detail');
    };

    const renderMobileList = () => (
        <div className="flex h-full min-h-0 flex-col" data-aipp-slot="config-mobile-list">
            {(showDropdownSearch || addButton) && (
                <div className="flex flex-shrink-0 items-center gap-2 px-4 pt-3 pb-2" data-aipp-slot="config-mobile-list-toolbar">
                    {showDropdownSearch ? (
                        <div className="relative min-w-0 flex-1">
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
                    ) : (
                        <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">共 {selectOptions?.length ?? 0} 项</span>
                    )}
                    {addButton && <div className="flex flex-shrink-0 items-center">{addButton}</div>}
                </div>
            )}
            <div className="min-h-0 flex-1 overflow-y-auto thin-scrollbar pb-[max(1rem,env(safe-area-inset-bottom,0px))]" data-aipp-slot="config-page-scroll">
                {showDropdownSearch && dropdownQuery.trim() && filteredSelectOptions.length === 0 && (
                    <p className="px-4 py-6 text-center text-sm text-muted-foreground">没有匹配的项目</p>
                )}
                <ul className="divide-y divide-border">
                    {filteredSelectOptions.map((option) => {
                        const selected = option.id === selectedOptionId;
                        return (
                            <li key={option.id} className={`flex items-center ${selected ? 'bg-muted/50' : ''}`}>
                                <button
                                    type="button"
                                    className="flex min-h-14 min-w-0 flex-1 items-center gap-3 py-3 pl-4 pr-2 text-left active:bg-muted/60"
                                    aria-current={selected ? 'true' : undefined}
                                    onClick={() => openDetail(option.id)}
                                >
                                    {option.icon && (
                                        <span className="flex h-5 w-5 flex-shrink-0 items-center justify-center text-muted-foreground [&>svg]:h-5 [&>svg]:w-5">
                                            {option.icon}
                                        </span>
                                    )}
                                    <span className="min-w-0 flex-1">
                                        <span className="block truncate text-sm font-medium text-foreground">{option.label}</span>
                                        {option.description && (
                                            <span className="mt-0.5 block line-clamp-2 text-xs text-muted-foreground">{option.description}</span>
                                        )}
                                    </span>
                                    {!option.trailing && <ChevronRight className="h-4 w-4 flex-shrink-0 text-muted-foreground" />}
                                </button>
                                {option.trailing && <div className="flex flex-shrink-0 items-center pl-2 pr-4">{option.trailing}</div>}
                            </li>
                        );
                    })}
                </ul>
            </div>
        </div>
    );

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

    if (useMasterDetail && !showEmptyState) {
        return (
            <div className="flex flex-col h-full min-h-0 max-w-none" data-aipp-config-view={showDetail ? 'detail' : 'list'}>
                {showDetail ? (
                    <div className="flex h-full min-h-0 flex-col">
                        {/* 只有移动平台的设置顶栏承接返回；桌面窄窗口/独立窗口自带返回行 */}
                        {!(sectionId && isMobilePlatform) && (
                            <button
                                type="button"
                                className="flex min-h-11 flex-shrink-0 items-center gap-1 px-2 text-sm text-muted-foreground"
                                onClick={() => setMobileView('list')}
                            >
                                <ChevronLeft className="h-4 w-4" />返回列表
                            </button>
                        )}
                        <div className="flex-1 min-h-0 overflow-y-auto thin-scrollbar px-4 pt-4 pb-6" data-aipp-slot="config-page-scroll" data-aipp-config-detail-scroll>
                            {content}
                        </div>
                    </div>
                ) : renderMobileList()}
            </div>
        );
    }

    return (
        <div className="flex flex-col h-full min-h-0 max-w-none">
            {/* 响应式下拉菜单 - 小屏幕时显示 */}
            {renderDropdownHeader()}

            {/* 主要内容区域 */}
            {showEmptyState ? (
                <div className="flex-1 min-h-0 overflow-y-auto thin-scrollbar px-4 py-6">
                    {emptyState}
                </div>
            ) : shouldShowDropdown ? (
                // 窄屏：滚动容器占满宽度、内边距放在滚动容器内部，滚动条贴住屏幕边缘
                <div className="flex-1 min-h-0 overflow-y-auto thin-scrollbar px-4 pb-6" data-aipp-slot="config-page-scroll">
                    {content}
                </div>
            ) : (
                <div className="flex-1 min-h-0 grid gap-6 px-4 py-6 grid-cols-12">
                    {/* 左侧列表 - 大屏幕时显示，列表内部独立滚动（标题/搜索栏固定） */}
                    <div className="col-span-12 lg:col-span-4 xl:col-span-4 2xl:col-span-3 min-h-0">
                        {sidebar}
                    </div>

                    {/* 右侧配置区域，独立滚动 */}
                    <div className="min-h-0 overflow-y-auto thin-scrollbar col-span-12 lg:col-span-8 xl:col-span-8 2xl:col-span-9" data-aipp-slot="config-page-scroll">
                        {content}
                    </div>
                </div>
            )}
        </div>
    );
};

export default memo(ConfigPageLayout);
