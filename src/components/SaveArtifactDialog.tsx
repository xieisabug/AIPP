import React, { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useForm } from "react-hook-form";
import { Button } from "@/components/ui/button";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Badge } from "@/components/ui/badge";
import { Form, FormControl, FormField, FormItem, FormLabel, FormMessage } from "@/components/ui/form";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { toast } from "sonner";
import EmojiPicker from "@/components/ui/emoji-picker";
import { getDefaultIcon, getEmojisByCategory } from "@/utils/emojiUtils";
import { ArtifactMetadata, AssistantBasicInfo } from "@/data/ArtifactCollection";
import { Sparkles, Database, Bot } from "lucide-react";

interface SaveArtifactDialogProps {
    isOpen: boolean;
    onClose: () => void;
    artifactType: string;
    code: string;
    initialDbId?: string;
    initialAssistantId?: number;
}

export default function SaveArtifactDialog({
    isOpen,
    onClose,
    artifactType,
    code,
    initialDbId,
    initialAssistantId,
}: SaveArtifactDialogProps) {
    const [isLoading, setIsLoading] = useState(false);
    const [isGeneratingMetadata, setIsGeneratingMetadata] = useState(false);
    const [assistants, setAssistants] = useState<AssistantBasicInfo[]>([]);

    const form = useForm({
        defaultValues: {
            name: "",
            icon: getDefaultIcon(),
            description: "",
            tags: "",
            db_id: initialDbId ?? "",
            assistant_id: initialAssistantId ? String(initialAssistantId) : "none",
        },
    });

    // 加载可用的助手列表
    useEffect(() => {
        if (isOpen) {
            invoke<AssistantBasicInfo[]>("artifact_get_assistants")
                .then(setAssistants)
                .catch(console.error);
        }
    }, [isOpen]);

    const handleSave = async (data: any) => {
        setIsLoading(true);
        try {
            const request = {
                name: data.name.trim(),
                icon: data.icon,
                description: data.description.trim(),
                artifact_type: artifactType,
                code,
                tags: data.tags.trim() || null,
                db_id: data.db_id.trim() || null,
                assistant_id: data.assistant_id && data.assistant_id !== "none" ? parseInt(data.assistant_id, 10) : null,
            };

            await invoke<number>("save_artifact_to_collection", { request });

            toast.success(`Artifact "${data.name}" 已保存到合集中`);

            form.reset();
            onClose();
        } catch (error) {
            console.error("保存失败:", error);
            toast.error("保存失败: " + error);
        } finally {
            setIsLoading(false);
        }
    };

    const handleCancel = () => {
        form.reset();
        onClose();
    };

    const handleGenerateMetadata = async () => {
        if (isGeneratingMetadata) return;

        setIsGeneratingMetadata(true);
        try {
            const metadata = await invoke<ArtifactMetadata>("generate_artifact_metadata", {
                artifactType,
                code,
            });

            const categoryKey = metadata.emoji_category || "objects";
            const emojis = getEmojisByCategory(categoryKey);
            const randomEmoji =
                emojis.length > 0 ? emojis[Math.floor(Math.random() * emojis.length)] : getDefaultIcon();

            // 填充表单字段
            form.setValue("name", metadata.name);
            form.setValue("description", metadata.description);
            form.setValue("tags", metadata.tags);
            form.setValue("icon", randomEmoji);

            toast.success("已根据代码内容自动生成相关信息");
        } catch (error) {
            console.error("智能填写失败:", error);
            toast.error("智能填写失败: " + error);
        } finally {
            setIsGeneratingMetadata(false);
        }
    };

    // 当对话框关闭时重置表单
    React.useEffect(() => {
        if (!isOpen) {
            form.reset();
        }
    }, [isOpen, form]);

    React.useEffect(() => {
        if (isOpen) {
            form.reset({
                name: "",
                icon: getDefaultIcon(),
                description: "",
                tags: "",
                db_id: initialDbId ?? "",
                assistant_id: initialAssistantId ? String(initialAssistantId) : "none",
            });
        }
    }, [isOpen, form, initialDbId, initialAssistantId]);

    return (
        <Dialog open={isOpen} onOpenChange={handleCancel}>
            <DialogContent className="sm:max-w-[525px] max-h-[80vh] overflow-y-auto">
                <DialogHeader>
                    <DialogTitle>保存 Artifact 到合集</DialogTitle>
                    <DialogDescription asChild>
                        <div>
                            <span>将当前的 {artifactType} artifact 保存到您的合集中，方便以后快速访问。</span>
                            <div className="mt-2">
                                <Button
                                    type="button"
                                    variant="outline"
                                    size="sm"
                                    onClick={handleGenerateMetadata}
                                    disabled={isGeneratingMetadata}
                                    className="gap-2"
                                >
                                    <Sparkles className={`h-4 w-4 ${isGeneratingMetadata ? "animate-pulse" : ""}`} />
                                    {isGeneratingMetadata ? "生成中..." : "智能填写"}
                                </Button>
                            </div>
                        </div>
                    </DialogDescription>
                </DialogHeader>

                <Form {...form}>
                    <form onSubmit={form.handleSubmit(handleSave)} className="space-y-6 py-4">
                        <FormField
                            control={form.control}
                            name="icon"
                            render={({ field }) => (
                                <FormItem className="space-y-3">
                                    <FormLabel className="flex items-center font-semibold text-sm text-foreground">
                                        图标
                                    </FormLabel>
                                    <FormControl>
                                        <EmojiPicker
                                            className="focus:ring-ring/20 focus:border-ring"
                                            value={field.value}
                                            onChange={field.onChange}
                                        />
                                    </FormControl>
                                    <FormMessage />
                                </FormItem>
                            )}
                        />

                        <FormField
                            control={form.control}
                            name="name"
                            rules={{ required: "请输入 artifact 名称" }}
                            render={({ field }) => (
                                <FormItem className="space-y-3">
                                    <FormLabel className="flex items-center font-semibold text-sm text-foreground">
                                        名称 *
                                    </FormLabel>
                                    <FormControl>
                                        <Input
                                            className="focus:ring-ring/20 focus:border-ring"
                                            placeholder="输入 artifact 名称"
                                            autoFocus
                                            {...field}
                                        />
                                    </FormControl>
                                    <FormMessage />
                                </FormItem>
                            )}
                        />

                        <FormField
                            control={form.control}
                            name="description"
                            render={({ field }) => (
                                <FormItem className="space-y-3">
                                    <FormLabel className="flex items-center font-semibold text-sm text-foreground">
                                        描述
                                    </FormLabel>
                                    <FormControl>
                                        <Textarea
                                            className="focus:ring-ring/20 focus:border-ring"
                                            placeholder="描述这个 artifact 的用途或特点..."
                                            rows={3}
                                            {...field}
                                        />
                                    </FormControl>
                                    <FormMessage />
                                </FormItem>
                            )}
                        />

                        <FormField
                            control={form.control}
                            name="tags"
                            render={({ field }) => (
                                <FormItem className="space-y-3">
                                    <FormLabel className="flex items-center font-semibold text-sm text-foreground">
                                        标签
                                    </FormLabel>
                                    <FormControl>
                                        <Input
                                            className="focus:ring-ring/20 focus:border-ring"
                                            placeholder="用逗号分隔多个标签，如: 图表,数据,可视化"
                                            {...field}
                                        />
                                    </FormControl>
                                    <FormMessage />
                                </FormItem>
                            )}
                        />

                        <FormField
                            control={form.control}
                            name="db_id"
                            render={({ field }) => (
                                <FormItem className="space-y-3">
                                    <FormLabel className="flex items-center gap-2 font-semibold text-sm text-foreground">
                                        <Database className="h-4 w-4" />
                                        数据库标识
                                    </FormLabel>
                                    <FormControl>
                                        <Input
                                            className="focus:ring-ring/20 focus:border-ring"
                                            placeholder="例如: my-todo-app（留空则不启用数据库）"
                                            {...field}
                                        />
                                    </FormControl>
                                    <p className="text-xs text-muted-foreground">
                                        为 artifact 分配独立的数据库，只能包含字母、数字、下划线和连字符
                                    </p>
                                    <FormMessage />
                                </FormItem>
                            )}
                        />

                        <FormField
                            control={form.control}
                            name="assistant_id"
                            render={({ field }) => (
                                <FormItem className="space-y-3">
                                    <FormLabel className="flex items-center gap-2 font-semibold text-sm text-foreground">
                                        <Bot className="h-4 w-4" />
                                        关联助手
                                    </FormLabel>
                                <Select onValueChange={field.onChange} value={field.value}>
                                    <FormControl>
                                        <SelectTrigger className="focus:ring-ring/20 focus:border-ring">
                                            <SelectValue placeholder="选择一个助手（可选）" />
                                        </SelectTrigger>
                                    </FormControl>
                                    <SelectContent>
                                        <SelectItem value="none">不关联助手</SelectItem>
                                        {assistants.map((assistant) => (
                                            <SelectItem key={assistant.id} value={String(assistant.id)}>
                                                <span className="flex items-center gap-2">
                                                    <span>{assistant.icon}</span>
                                                        <span>{assistant.name}</span>
                                                    </span>
                                                </SelectItem>
                                            ))}
                                        </SelectContent>
                                    </Select>
                                    <p className="text-xs text-muted-foreground">
                                        关联后 artifact 可以调用此助手进行 AI 对话
                                    </p>
                                    <FormMessage />
                                </FormItem>
                            )}
                        />

                        <FormItem className="space-y-3">
                            <FormLabel className="flex items-center font-semibold text-sm text-foreground">
                                类型
                            </FormLabel>
                            <FormControl>
                                <div className="px-3 py-2 bg-muted rounded-md">
                                    <Badge variant="secondary" className="text-sm">
                                        {artifactType}
                                    </Badge>
                                </div>
                            </FormControl>
                        </FormItem>

                        <FormItem className="space-y-3">
                            <FormLabel className="flex items-center font-semibold text-sm text-foreground">
                                代码预览
                            </FormLabel>
                            <FormControl>
                                <pre className="bg-muted p-3 rounded-md text-xs max-h-32 overflow-y-auto border">
                                    {code}
                                </pre>
                            </FormControl>
                        </FormItem>
                    </form>
                </Form>

                <DialogFooter>
                    <Button variant="outline" onClick={handleCancel}>
                        取消
                    </Button>
                    <Button onClick={form.handleSubmit(handleSave)} disabled={isLoading}>
                        {isLoading ? "保存中..." : "保存"}
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}
