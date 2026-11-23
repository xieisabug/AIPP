import React, { useState, useMemo } from 'react';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    CompactDialogFooter,
} from '@/components/ui/dialog';
import { Badge } from '@/components/ui/badge';
import { Input } from '@/components/ui/input';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { AlertTriangle, Eye, Loader2, Mic, Video, Search } from 'lucide-react';
import { cn } from '@/utils/utils';
import { PinyinFilter, usePinyinReady } from '../../utils/pinyinFilter';
import {
    ModelForSelection,
    ModelSelectionResponse,
} from './llmModelTypes';

interface ModelSelectionDialogProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    modelData: ModelSelectionResponse | null;
    onConfirm: (selectedModels: ModelForSelection[]) => void;
    loading: boolean;
}

const ModelSelectionDialog: React.FC<ModelSelectionDialogProps> = ({
    open,
    onOpenChange,
    modelData,
    onConfirm,
    loading
}) => {
    const [selectedModels, setSelectedModels] = useState<ModelForSelection[]>([]);
    const [searchQuery, setSearchQuery] = useState('');
    // 有搜索输入时才动态加载 pinyin-pro，加载完成后触发重新过滤
    const pinyinReady = usePinyinReady(searchQuery.trim().length > 0);

    React.useEffect(() => {
        if (modelData) {
            setSelectedModels(modelData.available_models.map(model =>
                !model.is_selected && model.available_modes?.length
                    ? { ...model, request_mode: model.available_modes[0] }
                    : model
            ));
        }
    }, [modelData]);

    React.useEffect(() => {
        if (!open) {
            setSearchQuery('');
        }
    }, [open]);

    const filteredModels = useMemo(() => {
        if (!searchQuery.trim()) return selectedModels;
        return selectedModels.filter(model =>
            PinyinFilter.matches(model.name, searchQuery) ||
            PinyinFilter.matches(model.code, searchQuery)
        );
    }, [selectedModels, searchQuery, pinyinReady]);

    const handleModelToggle = (modelCode: string, checked: boolean) => {
        setSelectedModels(prev =>
            prev.map(model =>
                model.code === modelCode && (!checked || model.request_mode !== "unsupported")
                    ? { ...model, is_selected: checked }
                    : model
            )
        );
    };

    const handleSelectAll = () => {
        setSelectedModels(prev =>
            prev.map(model => ({ ...model, is_selected: model.request_mode !== "unsupported" }))
        );
    };

    const handleDeselectAll = () => {
        setSelectedModels(prev =>
            prev.map(model => ({ ...model, is_selected: false }))
        );
    };

    const selectedCount = selectedModels.filter(m => m.is_selected).length;

    if (!modelData) return null;

    return (
        <Dialog open={open} onOpenChange={(next) => { if (!loading) onOpenChange(next); }}>
            <CompactDialogContent className="sm:max-w-2xl">
                <CompactDialogHeader
                    title="选择模型"
                    subtitle={`已选 ${selectedCount} / ${modelData.available_models.length}`}
                    description="勾选要启用的模型"
                    closeDisabled={loading}
                />

                <TooltipProvider>
                    <CompactDialogBody className="flex flex-col gap-3 overflow-hidden">
                        {/* 搜索框与批量操作 */}
                        <div className="flex shrink-0 items-center gap-2">
                            <div className="relative flex-1">
                                <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
                                <Input
                                    value={searchQuery}
                                    onChange={(e) => setSearchQuery(e.target.value)}
                                    placeholder="搜索模型..."
                                    className="h-8 pl-8 text-sm"
                                />
                            </div>
                            <Button
                                type="button"
                                variant="ghost"
                                size="sm"
                                className="h-8 px-2 text-xs"
                                onClick={handleSelectAll}
                            >
                                全选
                            </Button>
                            <Button
                                type="button"
                                variant="ghost"
                                size="sm"
                                className="h-8 px-2 text-xs"
                                onClick={handleDeselectAll}
                            >
                                取消全选
                            </Button>
                        </div>

                        {/* 缺失模型提醒 */}
                        {modelData.missing_models.length > 0 && (
                            <div className="shrink-0 rounded-lg border border-orange-200 bg-orange-50/80 p-3 dark:border-orange-800 dark:bg-orange-900/20">
                                <div className="mb-1.5 flex items-center gap-2 text-xs font-medium text-orange-800 dark:text-orange-200">
                                    <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
                                    以下模型未找到，将自动删除
                                </div>
                                <div className="flex flex-wrap gap-1">
                                    {modelData.missing_models.map((model, index) => (
                                        <Badge key={index} variant="outline" className="border-orange-300 text-xs text-orange-700 dark:border-orange-700 dark:text-orange-300">
                                            {model}
                                        </Badge>
                                    ))}
                                </div>
                            </div>
                        )}

                        {/* 模型列表 */}
                        <div className="grid min-h-0 max-h-[52vh] flex-1 grid-cols-1 content-start gap-1.5 overflow-y-auto pr-0.5 sm:grid-cols-2">
                            {filteredModels.map((model) => (
                                <label
                                    key={model.code}
                                    className={cn(
                                        "flex min-w-0 cursor-pointer items-center gap-2.5 rounded-md border border-border px-2.5 py-2 transition-colors hover:bg-muted/60",
                                        model.is_selected && "border-foreground/40 bg-muted/60",
                                    )}
                                >
                                    <Checkbox
                                        aria-label={model.name}
                                        checked={model.is_selected}
                                        disabled={model.request_mode === "unsupported" && !model.is_selected}
                                        onCheckedChange={(checked) =>
                                            handleModelToggle(model.code, checked === true)
                                        }
                                    />
                                    <Tooltip>
                                        <TooltipTrigger asChild>
                                            <span className="min-w-0 flex-1 truncate text-sm">
                                                {model.name}
                                            </span>
                                        </TooltipTrigger>
                                        <TooltipContent>
                                            <p>{model.name}</p>
                                        </TooltipContent>
                                    </Tooltip>
                                    <span className="flex shrink-0 gap-1 text-muted-foreground">
                                        {model.vision_support && <Eye className="h-3 w-3" />}
                                        {model.audio_support && <Mic className="h-3 w-3" />}
                                        {model.video_support && <Video className="h-3 w-3" />}
                                    </span>
                                </label>
                            ))}
                        </div>
                    </CompactDialogBody>
                </TooltipProvider>

                <CompactDialogFooter>
                    <span className="mr-auto text-xs text-muted-foreground">
                        已选择 {selectedCount} / {modelData.available_models.length} 个模型
                    </span>
                    <Button type="button" variant="outline" className="min-h-11 px-4 sm:min-h-9" disabled={loading} onClick={() => onOpenChange(false)}>
                        取消
                    </Button>
                    <Button type="button" className="min-h-11 px-5 sm:min-h-9" onClick={() => onConfirm(selectedModels)} disabled={loading}>
                        {loading && <Loader2 className="h-4 w-4 animate-spin" />}
                        {loading ? '更新中...' : '确认更新'}
                    </Button>
                </CompactDialogFooter>
            </CompactDialogContent>
        </Dialog>
    );
};

export default ModelSelectionDialog;
