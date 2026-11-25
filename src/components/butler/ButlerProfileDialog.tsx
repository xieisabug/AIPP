import { useState, type FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { ExternalLink, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { navigateMobileConfig } from "@/mobile/navigation";
import { Input } from "@/components/ui/input";
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    CompactDialogFooter,
} from "@/components/ui/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useFilteredModels, type ModelForSelect } from "@/hooks/useFilteredModels";
import AssistantProfileEditor from "./AssistantProfileEditor";
import ButlerAvatar from "./ButlerAvatar";
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
    /** 管家窗口：分成资料、形象、记忆三页。配置页编辑不传，仍是单页。 */
    butler?: boolean;
    /** 当前助手。资料页可据此打开设置并选中它；总管家没有对应条目时不传。 */
    assistantId?: number;
    /** 管家私聊或总管家会话。有值时记忆页可管理该助手的记忆；还没有私聊时记忆页只说明原因。 */
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

/** 设置窗口预先创建、平时是隐藏的。先让它在隐藏时选中助手，再显示，避免打开后从别的页或别的助手跳一次。 */
function waitForConfigAssistant(assistantId: number) {
    return new Promise<boolean>((resolve) => {
        let settled = false;
        const finish = (matched: boolean) => {
            if (settled) return;
            settled = true;
            window.clearTimeout(timer);
            void unlisten.then(off => off());
            resolve(matched);
        };
        const timer = window.setTimeout(() => finish(false), 1500);
        const unlisten = listen<{ assistantId?: number }>("config-assistant-ready", (event) => {
            if (Number(event.payload?.assistantId) === assistantId) finish(true);
        });
    });
}

async function openAssistantSettings(assistantId: number, mobile: boolean) {
    if (mobile) {
        navigateMobileConfig("assistant-config", String(assistantId));
        return;
    }
    const ready = waitForConfigAssistant(assistantId);
    const navigation = { menu: "assistant-config", subNav: String(assistantId) };
    await emit("config-navigate-to", navigation);
    const matched = await ready;
    await invoke("open_config_window");
    // 隐藏窗口有时要等显示后才处理事件。超时再发一次，已经选中时不会再发。
    if (!matched) await emit("config-navigate-to", navigation);
}

export default function ButlerProfileDialog({ name: initialName, appearance: initialAppearance, description: initialDescription, model: modelField,
    butler = false, assistantId, memoryConversationId, onClose, onSave, appearanceOnly = false }: Props) {
    const [name, setName] = useState(initialName);
    const [description, setDescription] = useState(initialDescription ?? "");
    const [pickedModel, setPickedModel] = useState<ModelForSelect | null>(null);
    const [appearance, setAppearance] = useState(initialAppearance ?? defaultAppearance);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState("");
    const [tab, setTab] = useState("profile");
    const isMobilePlatform = useIsMobilePlatform();
    const openSettings = () => {
        if (assistantId == null || saving) return;
        openAssistantSettings(assistantId, isMobilePlatform).catch(error => {
            toast.error(`打开设置失败：${error instanceof Error ? error.message : String(error)}`);
        });
    };
    const withDescription = !appearanceOnly && (initialDescription !== undefined || modelField !== undefined);
    const tabbed = !appearanceOnly && (butler || memoryConversationId !== undefined);
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

    const profileFields = !appearanceOnly && <>
        <div className="space-y-2">
            <label htmlFor="butler-profile-name" className="text-sm font-medium">名称</label>
            <Input autoComplete="off" id="butler-profile-name" value={name} onChange={event => setName(event.target.value)} maxLength={80} autoFocus />
        </div>
        {withDescription && <div className="space-y-2">
            <label htmlFor="assistant-profile-description" className="text-sm font-medium">描述（选填）</label>
            <Input autoComplete="off" id="assistant-profile-description" value={description} placeholder="简单介绍助手的用途"
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
    </>;
    const errorLine = error && (
        <p role="alert" className="mx-4 mb-2 shrink-0 break-words rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive sm:mx-5">
            {error}
        </p>
    );
    const footer = <CompactDialogFooter>
        <Button type="button" variant="outline" className="min-h-11 px-4 sm:min-h-9" disabled={saving} onClick={onClose}>取消</Button>
        <Button type="submit" className="min-h-11 px-5 sm:min-h-9" disabled={saving || !name.trim()}>{saving && <Loader2 className="size-4 animate-spin" />}{saving ? "正在保存…" : "保存"}</Button>
    </CompactDialogFooter>;
    const onSubmit = (event: FormEvent) => { event.preventDefault(); void save(); };

    const profileForm = <form onSubmit={onSubmit} className="flex min-h-0 flex-1 flex-col">
        <CompactDialogBody className="flex flex-col overflow-hidden">
            <AssistantProfileEditor name={name} appearance={appearance} onAppearanceChange={setAppearance} disabled={saving}>
                {profileFields}
            </AssistantProfileEditor>
        </CompactDialogBody>
        {errorLine}
        {footer}
    </form>;

    const title = appearanceOnly ? "助手头像" : withDescription || tabbed ? "编辑助手" : "名字与形象";
    const descriptionText = appearanceOnly
        ? "设置助手的形状、配色和配饰，保存后同步用于会话列表和管家联系人。"
        : tabbed
            ? "资料和形象一起保存。记忆的修改立即生效。"
            : "修改助手的基本信息，并搭配属于它的形象。";
    return <Dialog open onOpenChange={open => { if (!open && !saving) onClose(); }}>
        <CompactDialogContent className="h-[min(780px,88dvh,var(--aipp-vv-height,88dvh))] sm:max-w-[760px]">
            <CompactDialogHeader
                title={title}
                subtitle={initialName}
                description={descriptionText}
                showCloseButton={!saving}
            />
            {!tabbed ? profileForm : <Tabs value={tab} onValueChange={setTab} className="flex min-h-0 flex-1 flex-col gap-0">
                <div className="shrink-0 border-b border-border/60 px-4 py-2.5 sm:px-5">
                    <TabsList className="grid w-full grid-cols-3">
                        <TabsTrigger value="profile">资料</TabsTrigger>
                        <TabsTrigger value="appearance">形象</TabsTrigger>
                        <TabsTrigger value="memory">记忆</TabsTrigger>
                    </TabsList>
                </div>
                {/* 记忆页不放进这个表单：页内按钮会误触发表单。切到记忆时藏起保存栏。 */}
                <form hidden={tab === "memory"} onSubmit={onSubmit} className="flex min-h-0 flex-1 flex-col">
                    <TabsContent value="profile" className="mt-0 flex min-h-0 flex-1 flex-col overflow-y-auto px-4 py-4 sm:px-5">
                        <div className="space-y-4 px-1 pb-1">
                            <div className="flex justify-center py-2">
                                <ButlerAvatar id={0} chief size={88} appearance={{ ...appearance, motion: true }} />
                            </div>
                            {profileFields}
                        </div>
                        {assistantId != null && <div className="mt-auto px-1 pt-4 pb-1">
                            <Button
                                type="button"
                                variant="outline"
                                className="w-full justify-center gap-1.5"
                                data-aipp-slot="butler-profile-open-assistant-settings"
                                disabled={saving}
                                onClick={openSettings}
                            >
                                打开完整配置（提示词 / 工具 / 权限）
                                <ExternalLink className="size-3.5" />
                            </Button>
                        </div>}
                    </TabsContent>
                    <TabsContent value="appearance" className="mt-0 flex min-h-0 flex-1 flex-col overflow-hidden px-4 py-4 sm:px-5">
                        <AssistantProfileEditor name={name} appearance={appearance} onAppearanceChange={setAppearance} disabled={saving} />
                    </TabsContent>
                    {errorLine}
                    {footer}
                </form>
                <TabsContent value="memory" className="mt-0 min-h-0 flex-1 overflow-y-auto px-4 py-4 sm:px-5">
                    {memoryConversationId !== undefined
                        ? <ButlerMemoryPanel conversationId={memoryConversationId} scope="assistant" />
                        : <p className="text-sm text-muted-foreground">还没有私聊。开始对话后，可以在这里查看和管理记忆。</p>}
                </TabsContent>
            </Tabs>}
        </CompactDialogContent>
    </Dialog>;
}
