import React from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import * as z from "zod";
import { Button } from "../ui/button";
import { DialogHeader, DialogFooter, Dialog, DialogTrigger, DialogContent, DialogTitle, DialogDescription } from "../ui/dialog";
import { Input } from "../ui/input";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { Form, FormControl, FormField, FormItem, FormLabel, FormMessage } from "../ui/form";
import { AssistantType } from "../../types/assistant";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { AssistantDetail } from "../../data/Assistant";
import { Loader2, Plus } from "lucide-react";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { isMobileExecutableAssistantType } from "@/lib/mobileUnsupported";
import { useFilteredModels } from "@/hooks/useFilteredModels";
import AssistantProfileEditor, { PROFILE_DIALOG_CLASS, PROFILE_FOOTER_CLASS } from "@/components/butler/AssistantProfileEditor";
import { appearanceKey, defaultAppearance, type ButlerAppearance } from "@/components/butler/butlerAppearance";

interface AddAssistantDialogProps {
    assistantTypes: AssistantType[];
    onAssistantAdded: (assistantDetail: AssistantDetail) => void;
    triggerButtonProps?: React.ComponentProps<typeof Button>;
}

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

const AddAssistantDialog: React.FC<AddAssistantDialogProps> = ({
    assistantTypes,
    onAssistantAdded,
    triggerButtonProps,
}) => {
    const [openAddAssistantDialog, setOpenAddAssistantDialog] = React.useState<boolean>(false);
    const [appearance, setAppearance] = React.useState<ButlerAppearance>(defaultAppearance);
    const isMobilePlatform = useIsMobilePlatform();
    // 移动端只能新建可在本机执行的助手类型；已有记录不受影响
    const creatableAssistantTypes = isMobilePlatform
        ? assistantTypes.filter((type) => isMobileExecutableAssistantType(type.code))
        : assistantTypes;

    const formSchema = z.object({
        name: z.string().trim().min(1, "名称不能为空"),
        description: z.string(),
        assistantType: z.string(),
        model: z.string().min(1, "请选择模型"),
    });

    const form = useForm<z.infer<typeof formSchema>>({
        resolver: zodResolver(formSchema),
        defaultValues: {
            name: "",
            description: "",
            assistantType: "0",
            model: "",
        },
    });
    const { models, loading: modelsLoading, error: modelsError } = useFilteredModels(Number(form.watch("assistantType")), openAddAssistantDialog);

    const onOpenChange = (open: boolean) => {
        if (form.formState.isSubmitting) return;
        setOpenAddAssistantDialog(open);
        if (!open) {
            form.reset();
            setAppearance(defaultAppearance);
        }
    };

    const onSubmit = async (values: z.infer<typeof formSchema>) => {
        if (!models.some(model => `${model.code}%%${model.llm_provider_id}` === values.model)) {
            form.setError("model", { message: "所选模型已不可用，请重新选择" });
            return;
        }
        let assistantDetail: AssistantDetail;
        try {
            assistantDetail = await invoke<AssistantDetail>("add_assistant", {
                name: values.name,
                description: values.description,
                assistantType: parseInt(values.assistantType),
                modelSelection: values.model,
            });
        } catch (error) {
            toast.error("新增助手失败: " + error);
            return;
        }
        const id = assistantDetail.assistant.id;
        try {
            // 编辑器不提供动画开关，与编辑助手一致统一开启。
            await saveAppearance(id, { ...appearance, motion: true });
            toast.success("新增助手成功");
        } catch (error) {
            toast.error(`助手已创建，但形象保存失败（助手 ${id}）：${error instanceof Error ? error.message : String(error)}。可以稍后在编辑助手里重新设置`);
        }
        onAssistantAdded(assistantDetail);
        setOpenAddAssistantDialog(false);
        form.reset();
        setAppearance(defaultAppearance);
    };

    const submitting = form.formState.isSubmitting;
    return (
        <Dialog open={openAddAssistantDialog} onOpenChange={onOpenChange}>
            <DialogTrigger asChild>
                <Button {...triggerButtonProps}>
                    <Plus className="h-4 w-4" />
                </Button>
            </DialogTrigger>
            <DialogContent className={PROFILE_DIALOG_CLASS} showCloseButton={!submitting}>
                <DialogHeader className="shrink-0">
                    <DialogTitle>新增助手</DialogTitle>
                    <DialogDescription>填写助手的基本信息，并搭配属于它的形象。</DialogDescription>
                </DialogHeader>
                <Form {...form}>
                    <form onSubmit={form.handleSubmit(onSubmit)} className="flex min-h-0 flex-1 flex-col gap-4">
                        <AssistantProfileEditor name={form.watch("name")} appearance={appearance} onAppearanceChange={setAppearance} disabled={submitting}>
                            <FormField
                                control={form.control}
                                name="name"
                                render={({ field }) => (
                                    <FormItem>
                                        <FormLabel>名称</FormLabel>
                                        <FormControl>
                                            <Input {...field} placeholder="例如：写作助手" enterKeyHint="done" autoCapitalize="none" maxLength={80} />
                                        </FormControl>
                                        <FormMessage />
                                    </FormItem>
                                )}
                            />
                            <FormField
                                control={form.control}
                                name="description"
                                render={({ field }) => (
                                    <FormItem>
                                        <FormLabel>描述（选填）</FormLabel>
                                        <FormControl>
                                            <Input {...field} placeholder="简单介绍助手的用途" enterKeyHint="done" autoCapitalize="none" />
                                        </FormControl>
                                        <FormMessage />
                                    </FormItem>
                                )}
                            />
                            <FormField
                                control={form.control}
                                name="assistantType"
                                render={({ field }) => (
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
                                )}
                            />
                            <FormField control={form.control} name="model" render={({ field }) => <FormItem>
                                <FormLabel>{form.watch("assistantType") === "4" ? "模型 / 智能体提供商" : "模型"}</FormLabel>
                                <Select value={field.value} onValueChange={field.onChange} disabled={modelsLoading || !!modelsError}>
                                    <FormControl><SelectTrigger className="w-full"><SelectValue placeholder={modelsLoading ? "加载模型中…" : "选择模型"} /></SelectTrigger></FormControl>
                                    <SelectContent>{models.map(model => <SelectItem key={`${model.code}%%${model.llm_provider_id}`} value={`${model.code}%%${model.llm_provider_id}`}>{model.name}</SelectItem>)}</SelectContent>
                                </Select>
                                {modelsError && <p role="alert" className="text-sm text-destructive">{modelsError}</p>}
                                {!modelsLoading && !modelsError && models.length === 0 && <p className="text-sm text-muted-foreground">暂无可用模型，请先在提供商设置中配置。</p>}
                                <FormMessage />
                            </FormItem>} />
                        </AssistantProfileEditor>
                        <DialogFooter className={PROFILE_FOOTER_CLASS}>
                            <Button type="button" variant="outline" disabled={submitting} onClick={() => onOpenChange(false)}>取消</Button>
                            <Button type="submit" disabled={submitting || modelsLoading || !!modelsError || models.length === 0}>
                                {submitting && <Loader2 className="size-4 animate-spin" />}{submitting ? "创建中…" : "创建"}
                            </Button>
                        </DialogFooter>
                    </form>
                </Form>
            </DialogContent>
        </Dialog>
    );
};

export default AddAssistantDialog;
