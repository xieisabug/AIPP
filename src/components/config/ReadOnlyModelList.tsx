import React, { useState, useCallback, useEffect, useRef } from 'react';
import { Button } from '../ui/button';
import { Badge } from '../ui/badge';
import { Tag, ChevronDown, ChevronUp } from 'lucide-react';
import { invoke } from "@tauri-apps/api/core";
import { toast } from 'sonner';

interface ModelForSelection {
    name: string;
    code: string;
    description: string;
    vision_support: boolean;
    audio_support: boolean;
    video_support: boolean;
    is_selected: boolean;
}

interface ModelSelectionResponse {
    available_models: ModelForSelection[];
    missing_models: string[];
}

interface ReadOnlyModelListProps {
    llmProviderId: string;
    tags: string[];
    onTagsChange: (tags: string[]) => void;
    onFetchModels?: (modelData: ModelSelectionResponse) => void;
}

const ReadOnlyModelList: React.FC<ReadOnlyModelListProps> = ({
    llmProviderId,
    tags,
    onTagsChange,
    onFetchModels
}) => {
    const [isExpanded, setIsExpanded] = useState<boolean>(false);
    const [isFetchingModels, setIsFetchingModels] = useState<boolean>(false);
    const [shouldShowExpandButton, setShouldShowExpandButton] = useState<boolean>(false);
    const tagsContainerRef = useRef<HTMLDivElement>(null);

    // 获取模型列表
    const handleFetchModels = useCallback(async () => {
        setIsFetchingModels(true);
        try {
            const modelData = await invoke<ModelSelectionResponse>("preview_model_list", { 
                llmProviderId: parseInt(llmProviderId) 
            });
            
            if (onFetchModels) {
                onFetchModels(modelData);
            } else {
                // 如果没有 onFetchModels 回调，直接更新选中的模型
                const selectedModels = modelData.available_models
                    .filter(m => m.is_selected)
                    .map(m => m.name);
                onTagsChange(selectedModels);
                
                if (modelData.available_models.length > 0) {
                    toast.success(`获取到 ${modelData.available_models.length} 个可用模型`);
                } else {
                    toast.info("未获取到可用模型");
                }
            }
        } catch (e) {
            toast.error("获取模型列表失败: " + e);
        } finally {
            setIsFetchingModels(false);
        }
    }, [llmProviderId, onFetchModels, onTagsChange]);

    // 检测是否需要显示展开按钮
    useEffect(() => {
        if (tags.length > 0 && tagsContainerRef.current) {
            const container = tagsContainerRef.current;
            const containerHeight = container.scrollHeight;
            const twoAndHalfRowsHeight = 110;
            setShouldShowExpandButton(containerHeight > twoAndHalfRowsHeight);
            if (containerHeight <= twoAndHalfRowsHeight) {
                setIsExpanded(false);
            }
        } else {
            setShouldShowExpandButton(false);
            setIsExpanded(false);
        }
    }, [tags]);

    const toggleExpansion = useCallback(() => {
        setIsExpanded(!isExpanded);
    }, [isExpanded]);

    return (
        <div className="space-y-4">
            <div className="space-y-3">
                <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2 text-sm text-muted-foreground">
                        <Tag className="h-4 w-4" />
                        <span className="font-medium">
                            {tags.length > 0 ? `已配置模型 (${tags.length})` : "模型列表"}
                        </span>
                    </div>
                    <div className="flex items-center gap-2">
                        <Button
                            variant="outline"
                            size="sm"
                            onClick={handleFetchModels}
                            disabled={isFetchingModels}
                            className="h-6 px-2 text-xs hover:bg-muted hover:border-muted-foreground"
                        >
                            {isFetchingModels ? "获取中..." : "获取Model列表"}
                        </Button>
                        {shouldShowExpandButton && (
                            <Button
                                variant="ghost"
                                size="sm"
                                onClick={toggleExpansion}
                                className="h-6 px-2 text-xs text-muted-foreground hover:text-foreground hover:bg-muted"
                            >
                                {isExpanded ? (
                                    <>
                                        <ChevronUp className="h-3 w-3 mr-1" />
                                        收起
                                    </>
                                ) : (
                                    <>
                                        <ChevronDown className="h-3 w-3 mr-1" />
                                        展开
                                    </>
                                )}
                            </Button>
                        )}
                    </div>
                </div>

                {tags.length > 0 && (
                    <div className="relative">
                        <div
                            ref={tagsContainerRef}
                            className={`
                                flex flex-wrap gap-2 p-3 bg-muted rounded-lg border border-border 
                                transition-all duration-300 ease-in-out
                                ${shouldShowExpandButton && !isExpanded
                                    ? 'max-h-[110px] overflow-hidden'
                                    : 'max-h-none'
                                }
                            `}
                            style={{ minHeight: tags.length > 0 ? '60px' : undefined }}
                        >
                            {tags.map((tag, index) => (
                                <Badge
                                    key={index}
                                    variant="secondary"
                                    className="bg-muted text-foreground border-border hover:bg-muted/80 transition-colors px-3 py-1 text-sm"
                                >
                                    <span>{tag}</span>
                                </Badge>
                            ))}
                        </div>

                        {/* 渐变遮罩效果和底部展开区域 */}
                        {shouldShowExpandButton && !isExpanded && (
                            <>
                                <div className="absolute bottom-0 left-0 right-0 h-8 bg-gradient-to-t from-muted to-transparent pointer-events-none rounded-b-lg" />
                                <div
                                    onClick={toggleExpansion}
                                    className="absolute bottom-0 left-0 right-0 h-8 flex items-center justify-center cursor-pointer hover:bg-muted/80 rounded-b-lg transition-colors group"
                                    title="点击展开查看更多模型"
                                >
                                    <div className="flex items-center gap-1 text-xs text-muted-foreground group-hover:text-foreground">
                                        <ChevronDown className="h-3 w-3" />
                                        <span>展开更多</span>
                                    </div>
                                </div>
                            </>
                        )}

                        {/* 展开状态下的收起区域 */}
                        {shouldShowExpandButton && isExpanded && (
                            <div className="mt-2 pt-2 border-t border-border">
                                <div
                                    onClick={toggleExpansion}
                                    className="flex items-center justify-center cursor-pointer hover:bg-muted rounded-md py-1 transition-colors group"
                                    title="点击收起模型列表"
                                >
                                    <div className="flex items-center gap-1 text-xs text-muted-foreground group-hover:text-foreground">
                                        <ChevronUp className="h-3 w-3" />
                                        <span>收起</span>
                                    </div>
                                </div>
                            </div>
                        )}
                    </div>
                )}

                {tags.length === 0 && (
                    <div className="text-sm text-muted-foreground p-3 bg-muted rounded-lg border border-border text-center">
                        点击「获取Model列表」按钮获取可用模型
                    </div>
                )}
            </div>
        </div>
    );
};

export default React.memo(ReadOnlyModelList, (prevProps, nextProps) => {
    return (
        prevProps.llmProviderId === nextProps.llmProviderId &&
        prevProps.tags.length === nextProps.tags.length &&
        prevProps.tags.every((tag, index) => tag === nextProps.tags[index])
    );
});
