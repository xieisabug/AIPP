import React, { useCallback, useEffect, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import * as z from "zod";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { ChevronLeft, Loader2, Plus, X } from "lucide-react";
import { Button } from "../ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogTitle, DialogTrigger } from "../ui/dialog";
import { Input } from "../ui/input";
import { Textarea } from "../ui/textarea";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { Form, FormControl, FormField, FormItem, FormLabel, FormMessage } from "../ui/form";
import { ConfigFormField, type FieldOptionsData } from "../ConfigForm";
import { AssistantType } from "../../types/assistant";
import { AssistantDetail } from "../../data/Assistant";
import type { AssistantFormConfig } from "@/types/forms";
import type { LoadedPlugin } from "@/services/PluginRuntime";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { isMobileExecutableAssistantType } from "@/lib/mobileUnsupported";
import { useFilteredModels } from "@/hooks/useFilteredModels";
import { navigateMobileConfig } from "@/mobile/navigation";
import { cn } from "@/utils/utils";
import AssistantProfileEditor from "@/components/butler/AssistantProfileEditor";
import { appearanceKey, defaultAppearance, type ButlerAppearance } from "@/components/butler/butlerAppearance";
import { useAssistantSetupEditor, type AssistantSetupEditor } from "./assistant/useAssistantSetupEditor";

interface AddAssistantDialogProps {
    assistantTypes: AssistantType[];
    onAssistantAdded: (assistantDetail: AssistantDetail) => void;
    triggerButtonProps?: React.ComponentProps<typeof Button>;
    /** 已加载插件：提供扩展配置字段与助手类型插件的自定义字段。 */
    pluginList?: LoadedPlugin[];
    /** 第三步里「去配置 MCP / Skills」的跳转；不传时打开设置窗口对应页面。 */
    navigateTo?: (menuKey: string) => void;
}

type Step = 1 | 2 | 3;
const STEP_LABELS: Record<Step, string> = { 1: "基本信息", 2: "形象", 3: "高级与扩展配置" };
/** 留空时使用的提示词，与后端新建助手的默认值一致。 */
const DEFAULT_PROMPT = "You are a helpful assistant.";
/** 只有普通对话助手在第一步填写提示词。 */
const NORMAL_ASSISTANT_TYPE = "0";
const EMPTY_PLUGINS: LoadedPlugin[] = [];
// 第三步只有高级/扩展字段，不含模型、提供商与 MCP 下拉
const EMPTY_FIELD_DATA: FieldOptionsData = {
    models: [], modelsLoading: false, modelsError: null,
    providers: [], providersLoading: false, providersError: null,
    mcpServers: [], mcpServersLoading: false, mcpServersError: null,
};

/** 形象存放在 butler_appearance 功能配置里（按助手 id），与编辑助手、管家联系人共用。 */
async function saveAppearance(id: number, appearance: ButlerAppearance) {
    const list = await invoke<Array<{ feature_code: string; key: string; value: string }>>("get_all_feature_config");
    const current = Object.fromEntries(list.filter(item => item.feature_code === "butler_appearance").map(item => [item.key, item.value]));
    await invoke("save_feature_config", {
        featureCode: "butler_appearance",
        feature_code: "butler_appearance",
        config: { ...current, [appearanceKey(id)]: JSON.stringify(appearance) },
    });
}

async function openConfigMenu(menu: string, mobile: boolean) {
    if (mobile) {
        navigateMobileConfig(menu);
        return;
    }
    await invoke("open_config_window");
    window.setTimeout(() => { void emit("config-navigate-to", { menu }); }, 50);
}

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

/**
 * 新增助手向导：① 名称 / 描述 / 类型 / 模型 → ② 形象 → ③ 高级配置与扩展配置。
 *
 * 第二步「下一步」时创建助手并保存形象，第三步对这个助手复用编辑页的高级/扩展字段
 * （MCP、Skills、工作区等按助手 id 读写，需要真实助手）。完成时保存第三步的改动；
 * 未完成就关闭（取消、Esc、离开页面）会删除这个助手。
 */
const AddAssistantDialog: React.FC<AddAssistantDialogProps> = ({ triggerButtonProps, ...rest }) => {
    const [open, setOpen] = useState(false);
    const [busy, setBusy] = useState(false);
    return (
        <Dialog open={open} onOpenChange={(next) => { if (!busy) setOpen(next); }}>
            <DialogTrigger asChild>
                <Button {...triggerButtonProps}>
                    <Plus className="h-4 w-4" />
                </Button>
            </DialogTrigger>
            <DialogContent
                showCloseButton={false}
                // 向导有多步输入，点到外面不关闭；用关闭按钮或 Esc 退出
                onInteractOutside={(event) => event.preventDefault()}
                className="flex h-[min(780px,88dvh,var(--aipp-vv-height,88dvh))] flex-col gap-0 overflow-hidden p-0 sm:max-w-[760px] !max-h-[min(88dvh,var(--aipp-vv-height,88dvh))] top-[max(1rem,var(--aipp-vv-offset-top,0px))] translate-y-0 data-[state=open]:slide-in-from-top-2 sm:top-[50%] sm:translate-y-[-50%]"
                data-aipp-slot="new-assistant-wizard"
            >
                <WizardContent {...rest} busy={busy} setBusy={setBusy} onClose={() => setOpen(false)} />
            </DialogContent>
        </Dialog>
    );
};

const formSchema = z.object({
    name: z.string().trim().min(1, "名称不能为空"),
    description: z.string(),
    assistantType: z.string(),
    model: z.string().min(1, "请选择模型"),
    prompt: z.string(),
});

/** 普通对话助手的提示词（留空用默认值）；其他类型不设置。 */
const promptOf = (values: BasicValues) =>
    values.assistantType === NORMAL_ASSISTANT_TYPE ? (values.prompt.trim() ? values.prompt : DEFAULT_PROMPT) : undefined;
type BasicValues = z.infer<typeof formSchema>;

interface Draft {
    detail: AssistantDetail;
    assistantType: string;
    model: string;
}

const WizardContent: React.FC<Omit<AddAssistantDialogProps, "triggerButtonProps"> & {
    busy: boolean;
    setBusy: (busy: boolean) => void;
    onClose: () => void;
}> = ({ assistantTypes, onAssistantAdded, pluginList = EMPTY_PLUGINS, navigateTo, busy, setBusy, onClose }) => {
    const [step, setStep] = useState<Step>(1);
    const [appearance, setAppearance] = useState<ButlerAppearance>(defaultAppearance);
    const [draft, setDraft] = useState<Draft | null>(null);
    const isMobilePlatform = useIsMobilePlatform();
    // 移动端只能新建可在本机执行的助手类型；已有记录不受影响
    const creatableAssistantTypes = isMobilePlatform
        ? assistantTypes.filter((type) => isMobileExecutableAssistantType(type.code))
        : assistantTypes;

    const form = useForm<BasicValues>({
        resolver: zodResolver(formSchema),
        defaultValues: { name: "", description: "", assistantType: NORMAL_ASSISTANT_TYPE, model: "", prompt: "" },
    });
    const assistantType = form.watch("assistantType");
    const { models, loading: modelsLoading, error: modelsError } = useFilteredModels(Number(assistantType), true);

    // 卸载时（任何方式关闭）若未完成则删除草稿
    const draftRef = useRef<Draft | null>(null);
    const finishedRef = useRef(false);
    useEffect(() => () => {
        const pending = draftRef.current;
        if (pending && !finishedRef.current) {
            const id = pending.detail.assistant.id;
            invoke("delete_assistant", { assistantId: id }).catch((error) => {
                toast.error(`删除未完成的新助手失败（助手 ${id}）：${errorText(error)}`);
            });
        }
    }, []);
    const updateDraft = (next: Draft | null) => {
        draftRef.current = next;
        setDraft(next);
    };

    // 第三步里的跳转先完成向导，避免离开页面时把助手当成未完成删掉
    const navigateRef = useRef<(menu: string) => void>(() => {});
    const stableNavigate = useCallback((menu: string) => navigateRef.current(menu), []);
    const editor = useAssistantSetupEditor(draft?.detail ?? null, pluginList, stableNavigate);

    const goToAppearance = async () => {
        const valid = await form.trigger();
        if (!valid) return;
        const model = form.getValues("model");
        if (!models.some((item) => `${item.code}%%${item.llm_provider_id}` === model)) {
            form.setError("model", { message: "所选模型已不可用，请重新选择" });
            return;
        }
        setStep(2);
    };

    /** 创建助手（类型或模型改过时重建）并保存形象，进入第三步。 */
    const goToAdvanced = async () => {
        if (busy) return;
        const values = form.getValues();
        setBusy(true);
        try {
            let current = draftRef.current;
            if (current && (current.assistantType !== values.assistantType || current.model !== values.model)) {
                // 类型或模型变了：第三步的字段随之不同，删掉旧助手重新创建
                await invoke("delete_assistant", { assistantId: current.detail.assistant.id });
                updateDraft(null);
                current = null;
            }
            if (!current) {
                const detail = await invoke<AssistantDetail>("add_assistant", {
                    name: values.name,
                    description: values.description,
                    assistantType: parseInt(values.assistantType),
                    modelSelection: values.model,
                    prompt: promptOf(values),
                });
                current = { detail, assistantType: values.assistantType, model: values.model };
                updateDraft(current);
            }
            const id = current.detail.assistant.id;
            try {
                // 编辑器不提供动画开关，与编辑助手一致统一开启。
                await saveAppearance(id, { ...appearance, motion: true });
            } catch (error) {
                toast.error(`形象保存失败（助手 ${id}）：${errorText(error)}。可以稍后在编辑助手里重新设置`);
            }
            setStep(3);
        } catch (error) {
            toast.error(`新增助手失败：${errorText(error)}`);
        } finally {
            setBusy(false);
        }
    };

    const finish = async (): Promise<boolean> => {
        const current = draftRef.current;
        if (!current || busy) return false;
        setBusy(true);
        try {
            const values = form.getValues();
            const saved = await editor.save({ name: values.name.trim(), description: values.description, prompt: promptOf(values) });
            const detail = saved ?? await invoke<AssistantDetail>("get_assistant", { assistantId: current.detail.assistant.id });
            finishedRef.current = true;
            toast.success("新增助手成功");
            onAssistantAdded(detail);
            onClose();
            return true;
        } catch (error) {
            toast.error(`保存助手配置失败（助手 ${current.detail.assistant.id}）：${errorText(error)}`);
            return false;
        } finally {
            setBusy(false);
        }
    };
    navigateRef.current = (menu: string) => {
        void finish().then((done) => {
            if (!done) return;
            if (navigateTo) navigateTo(menu);
            else openConfigMenu(menu, isMobilePlatform).catch((error) => toast.error(`打开设置失败：${errorText(error)}`));
        });
    };

    const isAgent = assistantType === "4";
    const name = form.watch("name");

    return (
        <>
            {/* 紧凑标题栏：标题 + 当前步骤，底边是进度条 */}
            <div className="relative flex shrink-0 items-center gap-3 border-b border-border px-4 py-2.5 sm:px-5">
                <DialogTitle className="truncate text-base font-semibold">新增助手</DialogTitle>
                <span className="truncate text-xs text-muted-foreground" data-testid="wizard-step">
                    {step}/3 · {STEP_LABELS[step]}
                </span>
                <DialogDescription className="sr-only">填写助手的基本信息，搭配形象，再按需调整高级与扩展配置</DialogDescription>
                <DialogClose asChild>
                    <button
                        type="button"
                        aria-label="关闭"
                        disabled={busy}
                        className="ml-auto flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-50"
                    >
                        <X className="h-4 w-4" />
                    </button>
                </DialogClose>
                <div
                    aria-hidden
                    className="absolute bottom-[-1px] left-0 h-0.5 bg-foreground/70 transition-[width] duration-300"
                    style={{ width: `${(step / 3) * 100}%` }}
                />
            </div>

            <div className={cn(
                "flex min-h-0 flex-1 flex-col px-4 py-4 sm:px-5",
                step !== 2 && "overflow-y-auto overscroll-contain",
            )}>
                {step === 1 && (
                    <Form {...form}>
                        <form
                            className="mx-auto w-full max-w-xl space-y-4"
                            onSubmit={(event) => { event.preventDefault(); void goToAppearance(); }}
                        >
                            <FormField control={form.control} name="name" render={({ field }) => (
                                <FormItem>
                                    <FormLabel>名称</FormLabel>
                                    <FormControl>
                                        <Input autoComplete="off" {...field} autoFocus placeholder="例如：写作助手" enterKeyHint="next" autoCapitalize="none" maxLength={80} />
                                    </FormControl>
                                    <FormMessage />
                                </FormItem>
                            )} />
                            <FormField control={form.control} name="description" render={({ field }) => (
                                <FormItem>
                                    <FormLabel>描述（选填）</FormLabel>
                                    <FormControl>
                                        <Input autoComplete="off" {...field} placeholder="简单介绍助手的用途" enterKeyHint="next" autoCapitalize="none" />
                                    </FormControl>
                                    <FormMessage />
                                </FormItem>
                            )} />
                            <FormField control={form.control} name="assistantType" render={({ field }) => (
                                <FormItem>
                                    <FormLabel>类型</FormLabel>
                                    <Select onValueChange={value => { field.onChange(value); form.setValue("model", ""); }} value={field.value}>
                                        <FormControl>
                                            <SelectTrigger className="w-full">
                                                <SelectValue placeholder="选择助手类型" />
                                            </SelectTrigger>
                                        </FormControl>
                                        <SelectContent>
                                            <SelectGroup>
                                                {creatableAssistantTypes.map((type) => (
                                                    <SelectItem key={type.code} value={type.code.toString()}>
                                                        {type.name}
                                                    </SelectItem>
                                                ))}
                                            </SelectGroup>
                                        </SelectContent>
                                    </Select>
                                    <FormMessage />
                                </FormItem>
                            )} />
                            <FormField control={form.control} name="model" render={({ field }) => (
                                <FormItem>
                                    <FormLabel>{isAgent ? "模型 / 智能体提供商" : "模型"}</FormLabel>
                                    <Select value={field.value} onValueChange={field.onChange} disabled={modelsLoading || !!modelsError}>
                                        <FormControl>
                                            <SelectTrigger className="w-full">
                                                <SelectValue placeholder={modelsLoading ? "加载模型中…" : "选择模型"} />
                                            </SelectTrigger>
                                        </FormControl>
                                        <SelectContent>
                                            {models.map(model => (
                                                <SelectItem key={`${model.code}%%${model.llm_provider_id}`} value={`${model.code}%%${model.llm_provider_id}`}>
                                                    {model.name}
                                                </SelectItem>
                                            ))}
                                        </SelectContent>
                                    </Select>
                                    {modelsError && <p role="alert" className="text-sm text-destructive">{modelsError}</p>}
                                    {!modelsLoading && !modelsError && models.length === 0 && (
                                        <p className="text-sm text-muted-foreground">暂无可用模型，请先在提供商设置中配置。</p>
                                    )}
                                    <FormMessage />
                                </FormItem>
                            )} />
                            {assistantType === NORMAL_ASSISTANT_TYPE && (
                                <FormField control={form.control} name="prompt" render={({ field }) => (
                                    <FormItem>
                                        <FormLabel>提示词（选填）</FormLabel>
                                        <FormControl>
                                            <Textarea autoComplete="off"
                                                {...field}
                                                className="min-h-40 resize-y"
                                                placeholder={`描述助手的角色、语气和要遵守的规则。留空使用默认提示词：${DEFAULT_PROMPT}`}
                                                autoCapitalize="none"
                                            />
                                        </FormControl>
                                        <FormMessage />
                                    </FormItem>
                                )} />
                            )}
                            {draft && (
                                <p className="text-xs text-muted-foreground">修改类型或模型后，第三步的配置会按新的类型重新开始。</p>
                            )}
                            {/* 回车提交到下一步 */}
                            <button type="submit" hidden aria-hidden tabIndex={-1} />
                        </form>
                    </Form>
                )}

                {step === 2 && (
                    <AssistantProfileEditor name={name} appearance={appearance} onAppearanceChange={setAppearance} disabled={busy} />
                )}

                {step === 3 && <AdvancedStep editor={editor} isAgent={draft?.assistantType === "4"} />}
            </div>

            {/* 底部内边距 = 顶部 + 安全区；不用 .safe-bottom（未分层 CSS 会把 pb 覆盖成 0） */}
            <div className="flex shrink-0 items-center gap-2 border-t border-border px-4 pt-2.5 pb-[calc(0.625rem+env(safe-area-inset-bottom))] sm:px-5">
                {step === 1 ? (
                    <DialogClose asChild>
                        <Button type="button" variant="ghost" className="min-h-11 px-3 sm:min-h-9" disabled={busy}>取消</Button>
                    </DialogClose>
                ) : (
                    <Button type="button" variant="ghost" className="min-h-11 gap-1 px-3 sm:min-h-9" disabled={busy} onClick={() => setStep(step === 3 ? 2 : 1)}>
                        <ChevronLeft className="h-4 w-4" />
                        上一步
                    </Button>
                )}
                <div className="ml-auto flex items-center gap-3">
                    {step === 1 && (
                        <Button
                            type="button"
                            className="min-h-11 px-5 sm:min-h-9"
                            disabled={modelsLoading || !!modelsError || models.length === 0}
                            onClick={() => void goToAppearance()}
                        >
                            下一步
                        </Button>
                    )}
                    {step === 2 && (
                        <Button type="button" className="min-h-11 px-5 sm:min-h-9" disabled={busy} onClick={() => void goToAdvanced()}>
                            {busy && <Loader2 className="h-4 w-4 animate-spin" />}
                            下一步
                        </Button>
                    )}
                    {step === 3 && (
                        <Button type="button" className="min-h-11 px-5 sm:min-h-9" disabled={busy || !editor.ready} onClick={() => void finish()}>
                            {busy && <Loader2 className="h-4 w-4 animate-spin" />}
                            完成
                        </Button>
                    )}
                </div>
            </div>
        </>
    );
};

const AdvancedStep: React.FC<{ editor: AssistantSetupEditor; isAgent: boolean }> = ({ editor, isAgent }) => {
    if (editor.loadError) {
        return <p role="alert" className="text-sm text-destructive">{editor.loadError}</p>;
    }
    if (!editor.ready) {
        return (
            <div role="status" className="flex flex-1 items-center justify-center gap-2 text-sm text-muted-foreground">
                <Loader2 className="h-4 w-4 animate-spin" />
                正在读取助手配置…
            </div>
        );
    }
    const sections = [
        {
            id: "advanced",
            title: "高级配置",
            description: isAgent
                ? "按需调整智能体的运行参数、审批与工具，不修改则使用默认值。"
                : "按需调整模型参数、工具与运行设置，不修改则使用默认值。",
            fields: editor.advancedFields,
        },
        { id: "extension", title: "扩展配置", description: "配置已安装插件提供的设置项。", fields: editor.extensionFields },
    ].filter((section) => section.fields.some((item) => !item.config.hidden));

    return (
        <Form {...editor.form}>
            <div className="space-y-6" data-testid="wizard-advanced-step">
                <p className="text-xs text-muted-foreground">这些配置都可以跳过，创建后随时在助手设置里修改。</p>
                {sections.length === 0 && (
                    <p className="py-8 text-center text-sm text-muted-foreground">这个类型的助手没有需要额外设置的配置。</p>
                )}
                {sections.map((section) => (
                    <section key={section.id} aria-label={section.title} className="space-y-3">
                        <div className="space-y-1 border-b border-border pb-2">
                            <h3 className="text-sm font-semibold">{section.title}</h3>
                            <p className="text-xs text-muted-foreground">{section.description}</p>
                        </div>
                        <FieldGrid fields={section.fields} editor={editor} />
                    </section>
                ))}
            </div>
        </Form>
    );
};

const FieldGrid: React.FC<{ fields: AssistantFormConfig[]; editor: AssistantSetupEditor }> = ({ fields, editor }) => (
    <div className="grid grid-cols-1 gap-x-6 sm:grid-cols-2">
        {fields.map((item) => (
            <ConfigFormField
                key={item.key}
                field={item.config}
                name={item.key}
                control={editor.form.control}
                data={EMPTY_FIELD_DATA}
                assistantConfigApi={editor.assistantConfigApi}
            />
        ))}
    </div>
);

export default AddAssistantDialog;
