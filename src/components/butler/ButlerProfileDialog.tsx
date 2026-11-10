import { useState } from "react";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useFilteredModels } from "@/hooks/useFilteredModels";
import AssistantProfileEditor, { PROFILE_DIALOG_CLASS, PROFILE_FOOTER_CLASS } from "./AssistantProfileEditor";
import ButlerMemoryPanel from "./ButlerMemoryPanel";
import { defaultAppearance, type ButlerAppearance } from "./butlerAppearance";

/** 管家窗口编辑联系人时提供：类型只读展示，模型可改（与新增助手的字段一致）。 */
export interface ProfileModelField {
    assistantType: number;
    typeName: string;
    /** 当前模型，格式 `模型代码%%提供商 id`；没有模型时为空串。 */
    value: string;
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
    onSave: (name: string, appearance: ButlerAppearance, description?: string, model?: string) => Promise<void>;
}

function ModelSelect({ field, value, onChange }: { field: ProfileModelField; value: string; onChange: (value: string) => void }) {
    const { models, loading, error } = useFilteredModels(field.assistantType);
    const known = models.some(model => `${model.code}%%${model.llm_provider_id}` === value);
    return <div className="space-y-2">
        <label htmlFor="assistant-profile-model" className="text-sm font-medium">{field.assistantType === 4 ? "模型 / 智能体提供商" : "模型"}</label>
        <Select value={known ? value : ""} onValueChange={onChange} disabled={loading || !!error}>
            <SelectTrigger id="assistant-profile-model" className="w-full">
                <SelectValue placeholder={loading ? "加载模型中…" : "选择模型"} />
            </SelectTrigger>
            <SelectContent>{models.map(model => <SelectItem key={`${model.code}%%${model.llm_provider_id}`}
                value={`${model.code}%%${model.llm_provider_id}`}>{model.name}</SelectItem>)}</SelectContent>
        </Select>
        {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
        {!loading && !error && value && !known && <p className="text-sm text-muted-foreground">当前模型（{value.split("%%")[0]}）已不可用，请重新选择。</p>}
    </div>;
}

export default function ButlerProfileDialog({ name: initialName, appearance: initialAppearance, description: initialDescription, model: modelField,
    memoryConversationId, onClose, onSave, appearanceOnly = false }: Props) {
    const [name, setName] = useState(initialName);
    const [description, setDescription] = useState(initialDescription ?? "");
    const [model, setModel] = useState(modelField?.value ?? "");
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
            if (modelField) await onSave(name.trim(), next, description, model);
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
                    <ModelSelect field={modelField} value={model} onChange={setModel} />
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
