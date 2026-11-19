import { useCallback, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { ArrowRight, KeyRound, Plus, ShieldOff } from "lucide-react";
import { ConfigPageLayout, EmptyState, ListItemButton, SidebarList, ConfigPageLoading } from "@/components/common";
import type { SelectOption } from "@/components/common";
import ConfirmDialog from "@/components/ConfirmDialog";
import FormDialog from "@/components/FormDialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { PasswordInput } from "@/components/ui/password-input";
import { Textarea } from "@/components/ui/textarea";
import { useDataRefresh } from "@/hooks/useDataRefresh";
import type { VaultCapability, VaultConfigReference, VaultCreateInput, VaultItem, VaultUsageEntry } from "@/data/Vault";
import VaultGuide from "./vault/VaultGuide";
import VaultItemDetail, { type VaultItemPatch } from "./vault/VaultItemDetail";
import { VaultKeyInput } from "./vault/VaultItemEditDialog";
import { isSimpleVaultValue, keyError, suggestKey } from "./vault/vaultHelpers";

interface VaultConfigProps {
    navigateTo?: (menuKey: string) => void;
}

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));
const simpleValueWarning = "凭据过于简单可能导致普通内容被误遮盖，建议使用较长且独特的值。";

/**
 * 只在需要用户处理时出现：其他设备开启了同步、口令已更换，或有条目还没解密。
 * 「未设置同步口令」「同步正常」都是常态，不显示。
 */
function SyncNotice({ capability }: { capability: VaultCapability }) {
    const where = "「程序功能 → 数据」";
    const pending = capability.pendingFromOtherDevices;
    const text = capability.syncState === "mismatch"
        ? `同步口令已在其他设备更换，请在${where}输入新口令。`
        : pending > 0
            ? `有 ${pending} 个来自其他设备的条目待解锁，在${where}输入同步口令后可用。`
            : capability.syncState === "needs_passphrase"
                ? `其他设备已开启同步，在${where}输入同步口令后才能同步。`
                : null;
    if (!text) return null;
    return <p className="shrink-0 rounded-lg border border-border bg-muted/40 px-3 py-2 text-sm" data-aipp-slot="vault-sync-notice">{text}</p>;
}

interface CreateState {
    name: string;
    key: string;
    keyEdited: boolean;
    description: string;
    value: string;
}

const blankCreate = (): CreateState => ({ name: "", key: "", keyEdited: false, description: "", value: "" });

export default function VaultConfig({ navigateTo }: VaultConfigProps) {
    const [capability, setCapability] = useState<VaultCapability | null>(null);
    const [items, setItems] = useState<VaultItem[]>([]);
    const [selectedId, setSelectedId] = useState<string | null>(null);
    const [usage, setUsage] = useState<VaultUsageEntry[]>([]);
    const [search, setSearch] = useState("");
    const [createOpen, setCreateOpen] = useState(false);
    const [create, setCreate] = useState<CreateState>(blankCreate);
    const [createError, setCreateError] = useState<string | null>(null);
    /** 替换对话框里输入的新值；null 表示对话框关闭。 */
    const [replaceValue, setReplaceValue] = useState<string | null>(null);
    const [replaceError, setReplaceError] = useState<string | null>(null);
    const [deleteOpen, setDeleteOpen] = useState(false);
    const [referenceState, setReferenceState] = useState<{ itemId: string; list: VaultConfigReference[] } | null>(null);
    const createValueIsSimple = isSimpleVaultValue(create.value);
    /** 名称里没有英文时只能生成 entry 这类通用键，提示用户改成助手认得出的名字。 */
    const genericKey = /^entry(_\d+)?$/.test(create.key);
    const replaceValueIsSimple = isSimpleVaultValue(replaceValue ?? "");

    const selected = items.find((item) => item.id === selectedId) ?? items[0] ?? null;
    const keys = useMemo(() => items.map((item) => item.key), [items]);

    const refresh = useCallback(async (isCurrent: () => boolean) => {
        const nextCapability = await invoke<VaultCapability>("vault_capability");
        const nextItems = nextCapability.available ? await invoke<VaultItem[]>("vault_list") : [];
        if (!isCurrent()) return;
        setCapability(nextCapability);
        setItems(nextItems);
    }, []);
    const reload = useDataRefresh({ events: ["vault_changed"], refresh, initial: true });

    const selectedKey = selected?.id;
    const references = referenceState && referenceState.itemId === selectedKey ? referenceState.list : [];
    useDataRefresh({
        events: ["vault_changed"],
        identity: selectedKey,
        initial: true,
        paused: !selectedKey,
        refresh: async (isCurrent) => {
            if (!selectedKey) return;
            const entries = await invoke<VaultUsageEntry[]>("vault_usage", { id: selectedKey, limit: 20 });
            if (isCurrent()) setUsage(entries);
        },
    });
    // 引用来自 MCP / Provider 配置，它们变化时也要刷新（模型改了配置会显示为「配置已变化」）。
    const reloadReferences = useDataRefresh({
        events: ["vault_changed", "mcp_state_changed", "llm_provider_changed"],
        identity: selectedKey,
        initial: true,
        paused: !selectedKey,
        refresh: async (isCurrent) => {
            if (!selectedKey) return;
            const list = await invoke<VaultConfigReference[]>("vault_references", { id: selectedKey });
            if (isCurrent()) setReferenceState({ itemId: selectedKey, list });
        },
    });

    const confirmReference = async (reference: VaultConfigReference) => {
        if (!selectedKey || !reference.snapshot || !references.includes(reference)) return;
        try {
            await invoke("vault_confirm_reference", {
                itemId: selectedKey, kind: reference.kind, configId: reference.configId, expectedSnapshot: reference.snapshot,
            });
            toast.success(`已确认 ${reference.name} 使用此条目访问 ${reference.target}`);
        } catch (error) {
            toast.error(errorText(error));
        } finally {
            reloadReferences();
        }
    };

    const filtered = useMemo(() => {
        const query = search.trim().toLowerCase();
        if (!query) return items;
        return items.filter((item) => [item.name, item.key, item.description].some((text) => text.toLowerCase().includes(query)));
    }, [items, search]);

    const openCreate = useCallback(() => {
        setCreate(blankCreate());
        setCreateError(null);
        setCreateOpen(true);
    }, []);
    const closeCreate = () => {
        setCreateOpen(false);
        setCreate(blankCreate()); // 不在内存里留下值
    };

    const submitCreate = async () => {
        const name = create.name.trim();
        const problem = !name ? "请填写名称" : keyError(create.key, keys) ?? (create.value.trim() ? null : "请填写值");
        if (problem) {
            setCreateError(problem);
            return;
        }
        const input: VaultCreateInput = {
            name,
            key: create.key,
            description: create.description.trim(),
            value: create.value,
            allowedSites: [],
            syncEnabled: true,
        };
        try {
            const created = await invoke<VaultItem>("vault_create", { input });
            closeCreate();
            setSelectedId(created.id);
            reload();
            toast.success("已加密保存，之后不会再显示这个值");
        } catch (error) {
            setCreateError(errorText(error));
        }
    };

    const update = useCallback(async (patch: VaultItemPatch) => {
        if (!selected) return false;
        try {
            await invoke<VaultItem>("vault_update", { id: selected.id, input: patch });
            reload();
            return true;
        } catch (error) {
            toast.error(errorText(error));
            return false;
        }
    }, [selected, reload]);

    const submitReplace = async () => {
        if (!selected || replaceValue === null) return;
        if (!replaceValue.trim()) {
            setReplaceError("请填写新的值");
            return;
        }
        try {
            await invoke<VaultItem>("vault_replace_secret", { id: selected.id, value: replaceValue });
            setReplaceValue(null);
            reload();
            toast.success("值已替换");
        } catch (error) {
            setReplaceError(errorText(error));
        }
    };

    const confirmDelete = async () => {
        if (!selected) return;
        try {
            await invoke("vault_delete", { id: selected.id });
            setDeleteOpen(false);
            setSelectedId(null);
            reload();
            toast.success("已删除");
        } catch (error) {
            toast.error(errorText(error));
        }
    };

    const addButton = useMemo(() => (
        <Button aria-label="新建条目" onClick={openCreate} className="gap-2">
            <Plus className="h-4 w-4" />
        </Button>
    ), [openCreate]);

    const selectOptions: SelectOption[] = useMemo(() => items.map((item) => ({
        id: item.id,
        label: item.status === "locked" ? `${item.name}（待解锁）` : item.name,
    })), [items]);

    const appCredentialsLink = navigateTo ? (
        <button
            type="button"
            className="flex w-full items-center justify-between rounded-lg border border-dashed border-border px-3 py-2 text-left text-xs text-muted-foreground hover:bg-muted/50"
            onClick={() => navigateTo("mcp-config")}
        >
            <span>应用连接的凭据在「应用与工具」中管理</span>
            <ArrowRight className="h-3.5 w-3.5 shrink-0" />
        </button>
    ) : null;

    const sidebar = (
        <SidebarList
            title="凭据保险箱"
            icon={<KeyRound className="h-5 w-5" />}
            searchValue={search}
            onSearchChange={setSearch}
            searchPlaceholder="搜索条目..."
            addButton={addButton}
        >
            {filtered.map((item) => (
                <ListItemButton key={item.id} isSelected={selected?.id === item.id} onClick={() => setSelectedId(item.id)}>
                    <span className="min-w-0 flex-1 truncate">{item.name}</span>
                    {item.status === "locked" && <span className="shrink-0 text-xs text-muted-foreground">待解锁</span>}
                </ListItemButton>
            ))}
            {filtered.length === 0 && <p className="px-3 py-6 text-center text-xs text-muted-foreground">没有匹配的条目</p>}
        </SidebarList>
    );

    if (!capability) return <ConfigPageLoading />;

    if (!capability.available) {
        return (
            <ConfigPageLayout
                sidebar={null}
                content={null}
                showEmptyState
                emptyState={
                    <EmptyState
                        icon={<ShieldOff className="h-8 w-8 text-muted-foreground" />}
                        title="凭据保险箱不可用"
                        description={capability.reason ?? "此设备没有可用的系统安全存储，保险箱已停用（不会以明文保存）。"}
                    />
                }
            />
        );
    }

    const dialogs = (
        <>
            <FormDialog title="新建条目" isOpen={createOpen} onClose={closeCreate} onSubmit={submitCreate}>
                <div className="space-y-5">
                    <div className="space-y-2">
                        <Label htmlFor="vault-create-name">名称</Label>
                        <Input
                            id="vault-create-name"
                            placeholder="例如：GitHub 令牌、身份证号"
                            value={create.name}
                            onChange={(event) => {
                                const name = event.target.value;
                                setCreate((state) => ({ ...state, name, key: state.keyEdited ? state.key : suggestKey(name, keys) }));
                            }}
                        />
                    </div>
                    <div className="space-y-2">
                        <Label htmlFor="vault-create-key">键</Label>
                        <VaultKeyInput
                            id="vault-create-key"
                            value={create.key}
                            describedBy={genericKey ? "vault-create-key-hint" : undefined}
                            onChange={(key) => setCreate((state) => ({ ...state, key, keyEdited: true }))}
                        />
                        {genericKey && (
                            <p id="vault-create-key-hint" className="text-xs text-muted-foreground">
                                建议改成有意义的英文，如 id_card，助手更容易认出它。
                            </p>
                        )}
                    </div>
                    <div className="space-y-2">
                        <Label htmlFor="vault-create-value">值</Label>
                        <PasswordInput
                            id="vault-create-value"
                            autoComplete="off"
                            aria-describedby={createValueIsSimple ? "vault-create-value-warning" : undefined}
                            value={create.value}
                            onChange={(event) => setCreate((state) => ({ ...state, value: event.target.value }))}
                        />
                        {createValueIsSimple && <p id="vault-create-value-warning" className="text-xs text-amber-600 dark:text-amber-400">{simpleValueWarning}</p>}
                        <p className="text-xs text-muted-foreground">只填值本身，令牌不要带 Bearer 前缀。</p>
                    </div>
                    <div className="space-y-2">
                        <Label htmlFor="vault-create-description">给助手的说明（可选）</Label>
                        <Textarea
                            id="vault-create-description"
                            rows={2}
                            placeholder="例如：GitHub API 令牌，可读写我的仓库"
                            value={create.description}
                            onChange={(event) => setCreate((state) => ({ ...state, description: event.target.value }))}
                        />
                    </div>
                    {createError && <p role="alert" className="text-sm text-destructive">{createError}</p>}
                </div>
            </FormDialog>

            <FormDialog
                title={`替换「${selected?.name ?? ""}」`}
                isOpen={replaceValue !== null}
                onClose={() => setReplaceValue(null)}
                onSubmit={submitReplace}
            >
                <div className="space-y-4">
                    <p className="text-sm text-muted-foreground">旧值不会显示，请输入新的值。自动允许的网站保持不变。</p>
                    <div className="space-y-2">
                        <Label htmlFor="vault-replace-value">新的值</Label>
                        <PasswordInput
                            id="vault-replace-value"
                            autoComplete="off"
                            aria-describedby={replaceValueIsSimple ? "vault-replace-value-warning" : undefined}
                            value={replaceValue ?? ""}
                            onChange={(event) => setReplaceValue(event.target.value)}
                        />
                        {replaceValueIsSimple && <p id="vault-replace-value-warning" className="text-xs text-amber-600 dark:text-amber-400">{simpleValueWarning}</p>}
                    </div>
                    {replaceError && <p role="alert" className="text-sm text-destructive">{replaceError}</p>}
                </div>
            </FormDialog>

            <ConfirmDialog
                isOpen={deleteOpen}
                title="删除条目"
                confirmText={`删除「${selected?.name ?? ""}」后，助手将无法再使用它${selected?.syncEnabled ? "；开启同步时其他设备上的也会删除" : ""}。${references.length > 0 ? `以下配置正在使用它，删除后将无法工作：${references.map((r) => r.name).join("、")}。` : ""}聊天记录会保留。`}
                onConfirm={confirmDelete}
                onCancel={() => setDeleteOpen(false)}
            />
        </>
    );

    if (items.length === 0) {
        return (
            <>
                <ConfigPageLayout
                    sidebar={null}
                    content={null}
                    showEmptyState
                    emptyState={
                        <VaultGuide onCreate={openCreate}>
                            <SyncNotice capability={capability} />
                            {appCredentialsLink}
                        </VaultGuide>
                    }
                />
                {dialogs}
            </>
        );
    }

    return (
        <>
            <ConfigPageLayout
                sidebar={sidebar}
                content={
                    <div className="space-y-4">
                        <SyncNotice capability={capability} />
                        {selected && (
                            <VaultItemDetail
                                item={selected}
                                usage={selected.id === selectedKey ? usage : []}
                                takenKeys={keys}
                                syncReady={capability.syncState === "ready"}
                                references={selected.id === selectedKey ? references : []}
                                onConfirmReference={(reference) => void confirmReference(reference)}
                                onUpdate={update}
                                onReplace={() => {
                                    setReplaceError(null);
                                    setReplaceValue("");
                                }}
                                onDelete={() => setDeleteOpen(true)}
                            />
                        )}
                    </div>
                }
                selectOptions={selectOptions}
                selectedOptionId={selected?.id}
                onSelectOption={setSelectedId}
                selectPlaceholder="选择条目"
                addButton={addButton}
            />
            {dialogs}
        </>
    );
}
