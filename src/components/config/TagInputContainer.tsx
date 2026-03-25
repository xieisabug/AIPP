import React, { useCallback, useState } from 'react';
import TagInput from '../TagInput';
import { invoke } from "@tauri-apps/api/core";
import { toast } from 'sonner';
import {
    LLMModel,
    ModelSelectionResponse,
    ModelTagItem,
    supportsRequestModeToggle,
    toModelTagItem,
    toggleRequestMode,
} from './llmModelTypes';

interface TagInputContainerProps {
    llmProviderId: string;
    apiType: string;
    tags: ModelTagItem[];
    onTagsChange: (tags: ModelTagItem[]) => void;
    isExpanded?: boolean;
    onExpandedChange?: (expanded: boolean) => void;
    onFetchModels?: (modelData: ModelSelectionResponse) => void;
}

const TagInputContainer: React.FC<TagInputContainerProps> = ({
    llmProviderId,
    apiType,
    tags,
    onTagsChange,
    isExpanded: externalIsExpanded,
    onExpandedChange,
    onFetchModels
}) => {
    const [internalIsExpanded, setInternalIsExpanded] = useState<boolean>(false);
    const [isFetchingModels, setIsFetchingModels] = useState<boolean>(false);
    
    // 使用外部传入的展开状态，如果没有则使用内部状态
    const isExpanded = externalIsExpanded !== undefined ? externalIsExpanded : internalIsExpanded;
    const setIsExpanded = onExpandedChange || setInternalIsExpanded;

    // 添加模型
    const handleAddTag = useCallback((tag: string) => {
        invoke<LLMModel>('add_llm_model', { code: tag, llmProviderId: parseInt(llmProviderId) })
            .then((model) => {
                console.log("添加模型成功");
                onTagsChange([...tags, toModelTagItem(model)]);
            })
            .catch((e) => {
                console.log(e);
                toast.error('添加模型失败' + e);
            });
    }, [llmProviderId, tags, onTagsChange]);

    // 移除模型
    const handleRemoveTag = useCallback((tagToRemove: ModelTagItem, index: number) => {
        invoke('delete_llm_model', { code: tagToRemove.code, llmProviderId: parseInt(llmProviderId) })
            .then(() => {
                console.log("删除模型成功");
                onTagsChange(tags.filter((_, i) => i !== index));
            })
            .catch((e) => {
                console.log(e);
                toast.error('删除模型失败' + e);
            });
    }, [llmProviderId, tags, onTagsChange]);

    const handleToggleRequestMode = useCallback((tag: ModelTagItem, index: number) => {
        const requestMode = toggleRequestMode(tag.request_mode);
        invoke('update_llm_model_request_mode', {
            llmProviderId: parseInt(llmProviderId),
            modelCode: tag.code,
            requestMode,
        })
            .then(() => {
                const nextTags = [...tags];
                nextTags[index] = { ...tag, request_mode: requestMode };
                onTagsChange(nextTags);
            })
            .catch((e) => {
                console.log(e);
                toast.error('切换请求接口失败' + e);
            });
    }, [llmProviderId, onTagsChange, tags]);

    // 获取模型列表
    const handleFetchModels = useCallback(async () => {
        if (!onFetchModels) return;
        
        setIsFetchingModels(true);
        try {
            const modelData = await invoke<ModelSelectionResponse>("preview_model_list", { 
                llmProviderId: parseInt(llmProviderId) 
            });
            onFetchModels(modelData);
        } catch (e) {
            toast.error(
                "获取模型列表失败，请检查Endpoint和Api Key配置: " + e,
            );
        } finally {
            setIsFetchingModels(false);
        }
    }, [llmProviderId, onFetchModels]);

    return (
        <TagInput
            placeholder="输入自定义Model按回车确认"
            tags={tags}
            onAddTag={handleAddTag}
            onRemoveTag={handleRemoveTag}
            onToggleRequestMode={handleToggleRequestMode}
            showRequestModeToggle={supportsRequestModeToggle(apiType)}
            isExpanded={isExpanded}
            onExpandedChange={setIsExpanded}
            onFetchModels={onFetchModels ? handleFetchModels : undefined}
            isFetchingModels={isFetchingModels}
        />
    );
};

// 优化的比较函数，只在关键 props 变化时才重新渲染
export default React.memo(TagInputContainer, (prevProps, nextProps) => {
    return (
        prevProps.llmProviderId === nextProps.llmProviderId &&
        prevProps.apiType === nextProps.apiType &&
        prevProps.tags.length === nextProps.tags.length &&
        prevProps.tags.every((tag, index) =>
            tag.code === nextProps.tags[index]?.code &&
            tag.name === nextProps.tags[index]?.name &&
            tag.request_mode === nextProps.tags[index]?.request_mode
        ) &&
        prevProps.isExpanded === nextProps.isExpanded &&
        prevProps.onTagsChange === nextProps.onTagsChange &&
        prevProps.onExpandedChange === nextProps.onExpandedChange
    );
});
