import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useDataRefresh } from "@/hooks/useDataRefresh";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    CompactDialogFooter,
} from "@/components/ui/dialog";
import {
    ContextMenu,
    ContextMenuTrigger,
    ContextMenuContent,
    ContextMenuItem,
    ContextMenuSeparator,
} from "@/components/ui/context-menu";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useToast } from "@/hooks/use-toast";
import { useTheme } from "@/hooks/useTheme";
import { formatIconDisplay } from "@/utils/emojiUtils";
import { cn } from "@/utils/utils";
import { Search, Folder, PackageOpen, Pencil, Trash2, MessageSquareShare, RefreshCw } from "lucide-react";
import { WindowTopBar, dragRegionProps, useFramelessTitleBar, useWindowTitleBarLayout } from "@/components/window/WindowTitleBar";
import EditArtifactDialog from "@/components/EditArtifactDialog";
import { openConversationInChat } from "@/utils/openConversationInChat";
import {
    COLLECTION_SORT_LABELS,
    collectTags,
    collectTypes,
    filterAndSortCollections,
    type CollectionSort,
} from "./artifactCollectionFilters";

/** 收藏的本机来源：从哪个会话的哪个 Artifact 版本收藏而来。 */
interface CollectionSource {
    conversation_id: number;
    artifact_key: string;
    version: number;
}

/** 原会话里已有新版本的收藏。 */
interface CollectionSourceUpdate {
    id: number;
    current_version: number;
    latest_version: number;
}

function parseSource(raw?: string | null): CollectionSource | null {
    if (!raw) return null;
    try {
        const value = JSON.parse(raw) as Partial<CollectionSource>;
        return typeof value.conversation_id === "number" && typeof value.artifact_key === "string"
            ? (value as CollectionSource)
            : null;
    } catch {
        return null;
    }
}

interface ArtifactCollection {
    id: number;
    name: string;
    icon: string;
    description: string;
    artifact_type: string;
    tags?: string;
    created_time: string;
    last_used_time?: string;
    use_count: number;
    source?: string | null;
}

const ALL = "__all__";

/** 卡片上悬停/聚焦时出现的操作按钮（打开之外的常用操作，不必再进右键菜单）。 */
const CARD_ACTION_CLASS =
    "size-7 bg-background/90 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100";

export default function ArtifactCollectionsWindow() {
    useTheme("artifact_collections");
    // 桌面统一标题栏下标题在顶栏里，页面内不再重复大标题
    const titleBarLayout = useWindowTitleBarLayout();
    const framelessTitleBar = useFramelessTitleBar();

    const [artifacts, setArtifacts] = useState<ArtifactCollection[]>([]);
    const [updates, setUpdates] = useState<Map<number, CollectionSourceUpdate>>(new Map());
    const [syncingId, setSyncingId] = useState<number | null>(null);
    const [searchQuery, setSearchQuery] = useState("");
    const [typeFilter, setTypeFilter] = useState<string | null>(null);
    const [tagFilter, setTagFilter] = useState<string | null>(null);
    const [sort, setSort] = useState<CollectionSort>("frequent");
    const [isLoading, setIsLoading] = useState(true);
    const [showEditDialog, setShowEditDialog] = useState(false);
    const [editingArtifact, setEditingArtifact] = useState<ArtifactCollection | null>(null);
    const [showDeleteDialog, setShowDeleteDialog] = useState(false);
    const [deletingArtifact, setDeletingArtifact] = useState<ArtifactCollection | null>(null);
    const { toast } = useToast();

    // 加载所有 artifacts，以及原会话里已有新版本的收藏
    const loadArtifacts = async (isCurrent: () => boolean = () => true) => {
        try {
            setIsLoading(true);
            const [result, pending] = await Promise.all([
                invoke<ArtifactCollection[]>("get_artifacts_collection", { artifactType: null }),
                invoke<CollectionSourceUpdate[]>("get_collection_source_updates").catch((error) => {
                    console.warn("读取收藏来源更新失败:", error);
                    return [] as CollectionSourceUpdate[];
                }),
            ]);
            if (!isCurrent()) return;
            setArtifacts(result);
            setUpdates(new Map(pending.map((update) => [update.id, update])));
            setEditingArtifact((current) => current ? result.find((item) => item.id === current.id) ?? null : null);
            if (editingArtifact && !result.some((item) => item.id === editingArtifact.id)) setShowEditDialog(false);
            if (deletingArtifact && !result.some((item) => item.id === deletingArtifact.id)) {
                setShowDeleteDialog(false);
                setDeletingArtifact(null);
            }
        } catch (error) {
            console.error("加载 artifacts 失败:", error);
            toast({
                title: "加载失败",
                description: error as string,
                variant: "destructive",
            });
        } finally {
            if (isCurrent()) setIsLoading(false);
        }
    };

    const types = useMemo(() => collectTypes(artifacts), [artifacts]);
    const tags = useMemo(() => collectTags(artifacts), [artifacts]);
    // 筛选项在数据变化后可能已不存在（删除、改标签），此时按「全部」处理
    const activeType = typeFilter && types.includes(typeFilter) ? typeFilter : null;
    const activeTag = tagFilter && tags.includes(tagFilter) ? tagFilter : null;
    const filteredArtifacts = useMemo(
        () => filterAndSortCollections(artifacts, { query: searchQuery, type: activeType, tag: activeTag, sort }),
        [artifacts, searchQuery, activeType, activeTag, sort],
    );
    const isFiltering = Boolean(searchQuery.trim() || activeType || activeTag);

    useDataRefresh({
        events: ["artifact-collection-updated", "artifacts_collection_changed"],
        initial: true,
        refresh: async (isCurrent) => { await loadArtifacts(isCurrent); },
    });

    // 打开 artifact
    const openArtifact = async (artifact: ArtifactCollection) => {
        try {
            await invoke("open_artifact_window", { artifactId: artifact.id });
        } catch (error) {
            console.error("打开 artifact 失败:", error);
            toast({
                title: "打开失败",
                description: error as string,
                variant: "destructive",
            });
        }
    };

    const openSource = (source: CollectionSource) => {
        openConversationInChat(source.conversation_id).catch((error) =>
            toast({ title: "打开原会话失败", description: String(error), variant: "destructive" }),
        );
    };

    // 把收藏换成原会话里的最新版本
    const syncFromSource = async (artifact: ArtifactCollection) => {
        setSyncingId(artifact.id);
        try {
            const version = await invoke<number>("update_collection_from_source", { id: artifact.id });
            toast({ title: "已更新", description: `「${artifact.name}」已更新到原会话的 v${version}` });
            await loadArtifacts();
        } catch (error) {
            toast({ title: "更新失败", description: String(error), variant: "destructive" });
        } finally {
            setSyncingId(null);
        }
    };

    // 编辑 artifact
    const handleEdit = (artifact: ArtifactCollection) => {
        setEditingArtifact(artifact);
        setShowEditDialog(true);
    };

    // 删除 artifact
    const handleDelete = (artifact: ArtifactCollection) => {
        setDeletingArtifact(artifact);
        setShowDeleteDialog(true);
    };

    // 确认删除
    const confirmDelete = async () => {
        if (!deletingArtifact) return;

        try {
            await invoke("delete_artifact_collection", { id: deletingArtifact.id });

            toast({
                title: "删除成功",
                description: `已删除 "${deletingArtifact.name}"`,
            });

            setShowDeleteDialog(false);
            setDeletingArtifact(null);
            loadArtifacts();
        } catch (error) {
            console.error("删除失败:", error);
            toast({
                title: "删除失败",
                description: error as string,
                variant: "destructive",
            });
        }
    };

    return (
        <div className="flex flex-col h-screen bg-background" data-aipp-window="artifact_collections" data-aipp-slot="window-root">
            {/* 桌面统一标题栏：标题移到顶栏，与聊天 / 管家窗口头部同高 */}
            <WindowTopBar title="Artifacts 合集" icon={<PackageOpen />} data-aipp-slot="artifact-collections-titlebar">
                <span className="shrink-0 select-none text-xs text-muted-foreground" {...dragRegionProps(framelessTitleBar)}>共 {artifacts.length} 个</span>
            </WindowTopBar>
            <div className="flex min-h-0 flex-1 flex-col p-6">
            <div className="flex flex-col gap-3 mb-5" data-aipp-slot="artifact-collections-header">
                {!titleBarLayout && (
                <div className="flex items-center justify-between">
                    <div>
                        <h1 className="text-2xl font-bold">Artifacts 合集</h1>
                        <p className="text-muted-foreground">
                            管理您保存的所有 artifacts，共 {artifacts.length} 个项目
                        </p>
                    </div>
                </div>
                )}

                <div className="flex flex-wrap items-center gap-2" data-aipp-slot="artifact-collections-toolbar">
                    <div className="relative min-w-48 flex-1" data-aipp-slot="artifact-collections-search">
                        <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-muted-foreground" />
                        <Input
                            placeholder="搜索名称、描述或标签"
                            value={searchQuery}
                            onChange={(e) => setSearchQuery(e.target.value)}
                            className="pl-10"
                        />
                    </div>
                    {types.length > 1 && (
                        <Select value={activeType ?? ALL} onValueChange={(value) => setTypeFilter(value === ALL ? null : value)}>
                            <SelectTrigger className="w-32" aria-label="按类型筛选">
                                <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                                <SelectItem value={ALL}>全部类型</SelectItem>
                                {types.map((type) => (
                                    <SelectItem key={type} value={type}>{type}</SelectItem>
                                ))}
                            </SelectContent>
                        </Select>
                    )}
                    <Select value={sort} onValueChange={(value) => setSort(value as CollectionSort)}>
                        <SelectTrigger className="w-32" aria-label="排序方式">
                            <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                            {(Object.keys(COLLECTION_SORT_LABELS) as CollectionSort[]).map((key) => (
                                <SelectItem key={key} value={key}>{COLLECTION_SORT_LABELS[key]}</SelectItem>
                            ))}
                        </SelectContent>
                    </Select>
                </div>

                {tags.length > 0 && (
                    <div className="flex flex-wrap gap-1.5" role="group" aria-label="按标签筛选" data-aipp-slot="artifact-collections-tags">
                        {[null, ...tags].map((tag) => {
                            const active = activeTag === tag;
                            return (
                                <button
                                    key={tag ?? ALL}
                                    type="button"
                                    aria-pressed={active}
                                    onClick={() => setTagFilter(tag)}
                                    className={cn(
                                        "rounded-full border px-2.5 py-0.5 text-xs transition-colors",
                                        active
                                            ? "border-foreground bg-foreground text-background"
                                            : "border-border text-muted-foreground hover:bg-accent hover:text-accent-foreground",
                                    )}
                                >
                                    {tag ?? "全部"}
                                </button>
                            );
                        })}
                    </div>
                )}
            </div>

            <div className="flex-1 overflow-auto" data-aipp-slot="artifact-collections-content">
                {isLoading && artifacts.length === 0 ? (
                    <div className="flex items-center justify-center h-full">
                        <div className="text-center">
                            <Folder className="w-12 h-12 mx-auto mb-4 text-muted-foreground" />
                            <p className="text-muted-foreground">加载中...</p>
                        </div>
                    </div>
                ) : filteredArtifacts.length === 0 ? (
                    <div className="flex items-center justify-center h-full">
                        <div className="text-center">
                            <Folder className="w-12 h-12 mx-auto mb-4 text-muted-foreground" />
                            <p className="text-muted-foreground">
                                {isFiltering ? "没有找到匹配的 artifacts" : "还没有保存任何 artifacts"}
                            </p>
                        </div>
                    </div>
                ) : (
                    <div
                        className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 xl:grid-cols-8 2xl:grid-cols-10 gap-3"
                        data-aipp-slot="artifact-collections-grid"
                    >
                        {filteredArtifacts.map((artifact) => {
                            const source = parseSource(artifact.source);
                            const update = updates.get(artifact.id);
                            const syncing = syncingId === artifact.id;
                            return (
                            <ContextMenu key={artifact.id}>
                                <ContextMenuTrigger asChild>
                                    <div
                                        role="button"
                                        tabIndex={0}
                                        title={artifact.description || artifact.name}
                                        className="relative flex flex-col items-center p-2 cursor-pointer hover:bg-accent/50 focus-visible:bg-accent/50 focus-visible:outline-none rounded-lg transition-colors group"
                                        onClick={() => openArtifact(artifact)}
                                        onKeyDown={(event) => {
                                            if (event.target === event.currentTarget && (event.key === "Enter" || event.key === " ")) {
                                                event.preventDefault();
                                                void openArtifact(artifact);
                                            }
                                        }}
                                        data-aipp-slot="artifact-collections-card"
                                    >
                                        <div className="absolute right-1 top-1 flex gap-0.5" onClick={(event) => event.stopPropagation()}>
                                            {source && (
                                                <Button variant="ghost" iconLabel="回到原会话继续修改" className={CARD_ACTION_CLASS} onClick={() => openSource(source)}>
                                                    <MessageSquareShare className="size-3.5" />
                                                </Button>
                                            )}
                                            <Button variant="ghost" iconLabel="编辑" className={CARD_ACTION_CLASS} onClick={() => handleEdit(artifact)}>
                                                <Pencil className="size-3.5" />
                                            </Button>
                                            <Button variant="ghost" iconLabel="删除" className={CARD_ACTION_CLASS} onClick={() => handleDelete(artifact)}>
                                                <Trash2 className="size-3.5" />
                                            </Button>
                                        </div>

                                        <div className="mb-2 w-16 h-16">
                                            {(() => {
                                                const iconDisplay = formatIconDisplay(artifact.icon);
                                                return iconDisplay.isImage ? (
                                                    <img
                                                        src={iconDisplay.display}
                                                        alt={`Icon for ${artifact.name}`}
                                                        className="w-16 h-16 object-cover"
                                                    />
                                                ) : (
                                                    <span className="text-6xl">{iconDisplay.display}</span>
                                                );
                                            })()}
                                        </div>

                                        <div className="flex w-full flex-col items-center gap-1 text-center">
                                            <p className="w-full text-xs font-medium truncate">
                                                {artifact.name}
                                            </p>
                                            <div className="flex flex-wrap justify-center gap-1">
                                                <Badge variant="secondary" className="text-xs">
                                                    {artifact.artifact_type}
                                                </Badge>
                                                {update && (
                                                    <Badge asChild variant="outline" className="cursor-pointer text-xs hover:bg-accent disabled:opacity-60">
                                                        <button
                                                            type="button"
                                                            disabled={syncing}
                                                            title={`原会话已有 v${update.latest_version}（收藏的是 v${update.current_version}），点击更新`}
                                                            onClick={(event) => {
                                                                event.stopPropagation();
                                                                void syncFromSource(artifact);
                                                            }}
                                                            data-aipp-slot="artifact-collections-update"
                                                        >
                                                            <RefreshCw className={cn("size-3", syncing && "animate-spin")} />
                                                            v{update.latest_version}
                                                        </button>
                                                    </Badge>
                                                )}
                                            </div>
                                        </div>
                                    </div>
                                </ContextMenuTrigger>

                                <ContextMenuContent>
                                    <ContextMenuItem onClick={() => openArtifact(artifact)}>打开</ContextMenuItem>
                                    <ContextMenuItem onClick={() => handleEdit(artifact)}>编辑</ContextMenuItem>
                                    {source && (
                                        <ContextMenuItem onClick={() => openSource(source)}>
                                            回到原会话继续修改（v{source.version}）
                                        </ContextMenuItem>
                                    )}
                                    {update && (
                                        <ContextMenuItem disabled={syncing} onClick={() => void syncFromSource(artifact)}>
                                            更新到原会话的 v{update.latest_version}
                                        </ContextMenuItem>
                                    )}
                                    <ContextMenuSeparator />
                                    <ContextMenuItem variant="destructive" onClick={() => handleDelete(artifact)}>
                                        删除
                                    </ContextMenuItem>
                                    <ContextMenuSeparator />
                                    <ContextMenuItem disabled>使用次数: {artifact.use_count}</ContextMenuItem>
                                    <ContextMenuItem disabled>
                                        创建时间: {new Date(artifact.created_time).toLocaleDateString()}
                                    </ContextMenuItem>
                                </ContextMenuContent>
                            </ContextMenu>
                            );
                        })}
                    </div>
                )}
            </div>
            </div>

            <EditArtifactDialog
                isOpen={showEditDialog}
                artifact={editingArtifact}
                onClose={() => {
                    setShowEditDialog(false);
                    setEditingArtifact(null);
                }}
                onUpdated={loadArtifacts}
            />

            <Dialog open={showDeleteDialog} onOpenChange={setShowDeleteDialog}>
                <CompactDialogContent className="sm:max-w-md">
                    <CompactDialogHeader
                        title="确认删除"
                        description="此操作无法撤销"
                    />
                    <CompactDialogBody>
                        <p className="text-sm text-muted-foreground">
                            确定要删除 &quot;{deletingArtifact?.name}&quot; 吗？此操作无法撤销。
                        </p>
                    </CompactDialogBody>
                    <CompactDialogFooter>
                        <Button type="button" variant="outline" className="min-h-11 px-4 sm:min-h-9" onClick={() => setShowDeleteDialog(false)}>
                            取消
                        </Button>
                        <Button type="button" variant="destructive" className="min-h-11 px-5 sm:min-h-9" onClick={confirmDelete}>
                            删除
                        </Button>
                    </CompactDialogFooter>
                </CompactDialogContent>
            </Dialog>
        </div>
    );
}
