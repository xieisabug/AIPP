import { useState } from "react";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useFilteredModels, type ModelForSelect } from "@/hooks/useFilteredModels";
import AssistantProfileEditor, { PROFILE_DIALOG_CLASS, PROFILE_FOOTER_CLASS } from "./AssistantProfileEditor";
import ButlerMemoryPanel from "./ButlerMemoryPanel";
import { defaultAppearance, type ButlerAppearance } from "./butlerAppearance";

/** 助手第一个模型（assistant_model）：智能体助手只选了提供商时 `modelCode` 为空。 */
export interface ProfileModelChoice {
    modelCode: string;
    providerId: number;
}

/** 管家窗口编辑联系人时提供：类型只读展示，模型可改（与新增助手的字段一致）。 */
export interface ProfileModelField {
    assistantType: number;
    typeName: string;
    /** 当前模型；助手还没有模型时为 null。 */
    current: ProfileModelChoice | null;
}

interface Props {
    appearanceOnly?: boolean;
    name: string;
    appearance?: ButlerAppearance;
    description?: string;
    model?: ProfileModelField;
    /** 管家私聊或总管家会话：提供时多一个“记忆”页，查看和管理该助手的记忆。 */
    memoryConversationId?: number;
    onClose: () => void;
    /** `model` 只在用户换了模型时传入，未改动为 undefined。 */
    onSave: (name: string, appearance: ButlerAppearance, description?: string, model?: ProfileModelChoice) => Promise<void>;
}

const optionValue = (model: ModelForSelect) => `${model.code}%%${model.llm_provider_id}`;

/**
 * 智能体助手的提供商没有登记模型时，候选里只有一条代表提供商本身的选项（id 为 0、code 为 `acp`），
 * 新增助手选它会存成空模型代码（见后端 resolve_new_assistant_model），这里按同样规则反向对应。
 */
const isProviderOnly = (field: ProfileModelField, model: ModelForSelect) => field.assistantType === 4 && model.id === 0;

/** 助手当前模型对应的候选项：先按模型代码精确匹配；智能体助手再匹配同一提供商的“仅提供商”选项（如 Codex 的默认模型由其自身配置决定）。 */
export function findCurrentModelOption(models: ModelForSelect[], field: ProfileModelField): ModelForSelect | undefined {
    const current = field.current;
    if (!current || current.providerId <= 0) return undefined;
    const sameProvider = models.filter(model => model.llm_provider_id === current.providerId);
    return sameProvider.find(model => !isProviderOnly(field, model) && model.code === current.modelCode)
        ?? sameProvider.find(model => isProviderOnly(field, model));
}

export const toModelChoice = (field: ProfileModelField, model: ModelForSelect): ProfileModelChoice =>
    ({ modelCode: isProviderOnly(field, model) ? "" : model.code, providerId: model.llm_provider_id });

function ModelSelect({ field, picked, onPick }: { field: ProfileModelField; picked: ModelForSelect | null; onPick: (model: ModelForSelect | null) => void }) {
    const { models, loading, error } = useFilteredModels(field.assistantType);
    const current = findCurrentModelOption(models, field);
    const shown = picked ?? current;
    return <div className="space-y-2">
        <label htmlFor="assistant-profile-model" className="text-sm font-medium">{field.assistantType === 4 ? "模型 / 智能体提供商" : "模型"}</label>
        <Select value={shown ? optionValue(shown) : ""} disabled={loading || !!error} onValueChange={value => {
            const model = models.find(option => optionValue(option) === value);
            // 选回原来的模型等于没改，保存时不写模型。
            onPick(!model || (current && optionValue(current) === value) ? null : model);
        }}>
            <SelectTrigger id="assistant-profile-model" className="w-full">
                <SelectValue placeholder={loading ? "加载模型中…" : "选择模型"} />
            </SelectTrigger>
            <SelectContent>{models.map(model => <SelectItem key={optionValue(model)} value={optionValue(model)}>{model.name}</SelectItem>)}</SelectContent>
        </Select>
        {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
        {!loading && !error && field.current && !current && !picked && <p className="text-sm text-muted-foreground">
            当前模型（{field.current.modelCode || `提供商 ${field.current.providerId}`}）已不可用，请重新选择。
        </p>}
    </div>;
}

export default function ButlerProfileDialog({ name: initialName, appearance: initialAppearance, description: initialDescription, model: modelField,
    memoryConversationId, onClose, onSave, appearanceOnly = false }: Props) {
    const [name, setName] = useState(initialName);
    const [description, setDescription] = useState(initialDescription ?? "");
    const [pickedModel, setPickedModel] = useState<ModelForSelect | null>(null);
    const [appearance, setAppearance] = useState(initialAppearance ?? defaultAppearance);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState("");
    const withDescription = !appearanceOnly && (initialDescription !== undefined || modelField !== undefined);
    const save = async () => {
        if (!name.trim() || saving) return;
        setSaving(true);
        setError("");
        try {
            // 编辑器不再提供动画开关，保存时统一开启（兼容旧配置里关掉的情况）。
            const next = { ...appearance, motion: true };
            if (modelField) await onSave(name.trim(), next, description, pickedModel ? toModelChoice(modelField, pickedModel) : undefined);
            else if (initialDescription === undefined) await onSave(name.trim(), next);
            else await onSave(name.trim(), next, description);
            onClose();
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : String(cause));
        } finally {
            setSaving(false);
        }
    };

    const profileForm = <form onSubmit={event => { event.preventDefault(); void save(); }} className="flex min-h-0 flex-1 flex-col gap-4">
        <AssistantProfileEditor name={name} appearance={appearance} onAppearanceChange={setAppearance} disabled={saving}>
            {!appearanceOnly && <>
                <div className="space-y-2">
                    <label htmlFor="butler-profile-name" className="text-sm font-medium">名称</label>
                    <Input id="butler-profile-name" value={name} onChange={event => setName(event.target.value)} maxLength={80} autoFocus />
                </div>
                {withDescription && <div className="space-y-2">
                    <label htmlFor="assistant-profile-description" className="text-sm font-medium">描述（选填）</label>
                    <Input id="assistant-profile-description" value={description} placeholder="简单介绍助手的用途"
                        onChange={event => setDescription(event.target.value)} />
                </div>}
                {modelField && <>
                    <div className="space-y-2">
                        <label htmlFor="assistant-profile-type" className="text-sm font-medium">类型</label>
                        <Input id="assistant-profile-type" value={modelField.typeName} disabled readOnly />
                        <p className="text-xs text-muted-foreground">类型创建后不能修改。</p>
                    </div>
                    <ModelSelect field={modelField} picked={pickedModel} onPick={setPickedModel} />
                </>}
            </>}
        </AssistantProfileEditor>
        {error && <p role="alert" className="shrink-0 break-words text-sm text-destructive">{error}</p>}
        <DialogFooter className={PROFILE_FOOTER_CLASS}>
            <Button type="button" variant="outline" disabled={saving} onClick={onClose}>取消</Button>
            <Button type="submit" disabled={saving || !name.trim()}>{saving && <Loader2 className="size-4 animate-spin" />}{saving ? "正在保存…" : "保存"}</Button>
        </DialogFooter>
    </form>;

    const tabbed = memoryConversationId !== undefined && !appearanceOnly;
    const title = appearanceOnly ? "助手头像" : withDescription ? "编辑助手" : "名字与形象";
    return <Dialog open onOpenChange={open => { if (!open && !saving) onClose(); }}>
        <DialogContent className={PROFILE_DIALOG_CLASS} showCloseButton={!saving}>
            <DialogHeader className="shrink-0">
                <DialogTitle>{title}</DialogTitle>
                <DialogDescription>{appearanceOnly ? "设置助手的形状、配色和配饰，保存后同步用于会话列表和管家联系人。" : "修改助手的基本信息，并搭配属于它的形象。"}</DialogDescription>
            </DialogHeader>
            {!tabbed ? profileForm : <Tabs defaultValue="profile" className="min-h-0 flex-1 gap-4">
                <TabsList className="shrink-0">
                    <TabsTrigger value="profile" className="px-4">{withDescription ? "资料与形象" : "名字与形象"}</TabsTrigger>
                    <TabsTrigger value="memory" className="px-4">记忆</TabsTrigger>
                </TabsList>
                {/* 资料页自己管理滚动（右侧表单滚动、底部操作栏固定）。 */}
                <TabsContent value="profile" className="flex min-h-0 flex-col">{profileForm}</TabsContent>
                {/* 记忆的修改立即生效，不跟随资料页的保存按钮。 */}
                <TabsContent value="memory" className="min-h-0 overflow-y-auto pr-1">
                    <ButlerMemoryPanel conversationId={memoryConversationId} scope="assistant" />
                </TabsContent>
            </Tabs>}
        </DialogContent>
    </Dialog>;
}
