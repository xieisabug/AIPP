import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
    ArrowLeft,
    BookOpen,
    Download,
    ExternalLink,
    FolderOpen,
    Loader2,
    RefreshCw,
    ShieldAlert,
    Sparkles,
} from 'lucide-react';
import { toast } from 'sonner';

import { Badge } from '../ui/badge';
import { Button } from '../ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '../ui/card';
import { Checkbox } from '../ui/checkbox';
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from '../ui/dialog';
import { Input } from '../ui/input';
import { Separator } from '../ui/separator';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../ui/tabs';

interface InstallStep {
    title: string;
    description: string;
}

interface SkillInstallGuideDialogProps {
    isOpen: boolean;
    onClose: () => void;
    onSkillInstalled?: () => void;
}

interface SkillInstallRecipeSource {
    type: 'github' | 'zip';
    repo?: string | null;
    ref: string;
    url?: string | null;
}

interface SkillInstallRecipeDir {
    from: string;
    to: string;
}

interface SkillMetadata {
    name?: string | null;
    description?: string | null;
    version?: string | null;
    author?: string | null;
    tags: string[];
    requires_files: string[];
}

interface SkillInstallPlanSkill {
    from: string;
    to: string;
    display_name: string;
    detected_entry_file: string;
    normalized_entry_file: string;
    will_replace: boolean;
    metadata: SkillMetadata;
    preview?: string | null;
}

interface SkillArchiveInspection {
    source: SkillInstallRecipeSource;
    source_label: string;
    download_url: string;
    target_directory: string;
    skills: SkillInstallPlanSkill[];
}

interface SkillArchiveInstallResult {
    source: SkillInstallRecipeSource;
    source_label: string;
    download_url: string;
    target_directory: string;
    installed_skills: SkillInstallPlanSkill[];
}

interface OfficialSkill {
    id: string;
    name: string;
    description: string;
    version?: string | null;
    source?: SkillInstallRecipeSource | null;
    dirs?: SkillInstallRecipeDir[] | null;
    download_url?: string | null;
    source_url?: string | null;
}

type FetchStatus = 'idle' | 'loading' | 'success' | 'error' | 'timeout';
type InstallTab = 'recommended' | 'remote' | 'manual';
type PendingActionMode = 'inspect' | 'install';

interface SourceRequest {
    source: SkillInstallRecipeSource;
    dirs?: SkillInstallRecipeDir[];
    useProxy: boolean;
    title: string;
    description?: string;
    sourceUrl?: string | null;
}

interface PendingActionError {
    mode: PendingActionMode;
    request: SourceRequest;
    message: string;
}

const installSteps: InstallStep[] = [
    {
        title: '步骤一：打开 Skills 文件夹',
        description: '点击“打开 Skills 文件夹”，或直接进入 ~/.agents/skills。',
    },
    {
        title: '步骤二：准备 Skill 目录',
        description: '确认目录中包含 SKILL.md 入口文件。',
    },
    {
        title: '步骤三：复制到 Skills 目录',
        description: '将完整的 Skill 目录复制进去，保留 references、rules、assets 等附属文件。',
    },
    {
        title: '步骤四：回到 AIPP 扫描并启用',
        description: '点击“扫描 Skills”，然后在列表中启用新安装的 Skill。',
    },
];

function buildSourceKey(source: SkillInstallRecipeSource): string {
    if (source.type === 'github') {
        return `github:${source.repo ?? ''}#${source.ref}`;
    }
    return `zip:${source.url ?? ''}`;
}

function buildSkillSelectionKey(skill: SkillInstallPlanSkill): string {
    return `${skill.from}=>${skill.to}`;
}

function buildSelectionMap(skills: SkillInstallPlanSkill[]): Record<string, boolean> {
    return skills.reduce<Record<string, boolean>>((acc, skill) => {
        acc[buildSkillSelectionKey(skill)] = true;
        return acc;
    }, {});
}

function getSourceBadgeLabel(source: SkillInstallRecipeSource): string {
    return source.type === 'github' ? 'GitHub ZIP' : 'ZIP 链接';
}

function getSourceSummary(source: SkillInstallRecipeSource): string {
    if (source.type === 'github') {
        return `${source.repo ?? ''}#${source.ref}`;
    }
    return source.url ?? '';
}

function resolveOfficialSkillSource(skill: OfficialSkill): SkillInstallRecipeSource | null {
    if (skill.source) {
        return skill.source;
    }
    if (skill.download_url) {
        return {
            type: 'zip',
            ref: 'main',
            url: skill.download_url,
        };
    }
    return null;
}

function matchesOfficialSkillSearch(skill: OfficialSkill, query: string): boolean {
    const normalizedQuery = query.trim().toLowerCase();
    if (!normalizedQuery) {
        return true;
    }

    const source = resolveOfficialSkillSource(skill);
    const searchableText = [
        skill.id,
        skill.name,
        skill.description,
        skill.version ?? '',
        source ? getSourceSummary(source) : '',
    ]
        .join('\n')
        .toLowerCase();

    return searchableText.includes(normalizedQuery);
}

function parseCustomSourceInput(
    rawInput: string,
): { source: SkillInstallRecipeSource; sourceUrl: string } {
    const input = rawInput.trim();
    if (!input) {
        throw new Error('请输入 GitHub 仓库或 ZIP 链接');
    }

    const repoPattern = /^([A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+)(?:#([^\s]+))?$/;
    const repoMatch = input.match(repoPattern);
    if (repoMatch) {
        const repo = repoMatch[1];
        const gitRef = repoMatch[2]?.trim() || 'main';
        return {
            source: {
                type: 'github',
                repo,
                ref: gitRef,
            },
            sourceUrl: `https://github.com/${repo}/tree/${gitRef}`,
        };
    }

    let url: URL;
    try {
        url = new URL(input);
    } catch {
        throw new Error('请输入合法的 GitHub 仓库（owner/repo 或 GitHub URL）或 ZIP 链接');
    }

    if (!['http:', 'https:'].includes(url.protocol)) {
        throw new Error('仅支持 http 或 https 的 GitHub / ZIP 链接');
    }

    if (url.hostname === 'github.com' && !url.pathname.toLowerCase().endsWith('.zip')) {
        const segments = url.pathname.split('/').filter(Boolean);
        if (segments.length < 2) {
            throw new Error('GitHub 链接至少需要包含 owner/repo');
        }

        const owner = segments[0];
        const repo = segments[1].replace(/\.git$/i, '');
        let gitRef = 'main';

        if (segments[2] === 'tree' && segments[3]) {
            gitRef = decodeURIComponent(segments[3]);
        } else if (url.hash.length > 1) {
            gitRef = decodeURIComponent(url.hash.slice(1));
        } else if (url.searchParams.get('ref')) {
            gitRef = url.searchParams.get('ref') || 'main';
        }

        return {
            source: {
                type: 'github',
                repo: `${owner}/${repo}`,
                ref: gitRef,
            },
            sourceUrl: `https://github.com/${owner}/${repo}/tree/${gitRef}`,
        };
    }

    return {
        source: {
            type: 'zip',
            ref: 'main',
            url: input,
        },
        sourceUrl: input,
    };
}

const SkillInstallGuideDialog: React.FC<SkillInstallGuideDialogProps> = ({
    isOpen,
    onClose,
    onSkillInstalled,
}) => {
    const [activeTab, setActiveTab] = useState<InstallTab>('recommended');
    const [isOpeningFolder, setIsOpeningFolder] = useState(false);
    const [officialSkills, setOfficialSkills] = useState<OfficialSkill[]>([]);
    const [officialSearch, setOfficialSearch] = useState('');
    const [fetchStatus, setFetchStatus] = useState<FetchStatus>('idle');
    const [fetchError, setFetchError] = useState('');
    const [isFetchingWithProxy, setIsFetchingWithProxy] = useState(false);
    const [sourceInput, setSourceInput] = useState('');
    const [sourceInputError, setSourceInputError] = useState('');
    const [inspection, setInspection] = useState<SkillArchiveInspection | null>(null);
    const [inspectionRequest, setInspectionRequest] = useState<SourceRequest | null>(null);
    const [selectedSkills, setSelectedSkills] = useState<Record<string, boolean>>({});
    const [isInspecting, setIsInspecting] = useState(false);
    const [isInstalling, setIsInstalling] = useState(false);
    const [pendingSourceKey, setPendingSourceKey] = useState<string | null>(null);
    const [actionError, setActionError] = useState<PendingActionError | null>(null);

    const resetTransientState = useCallback(() => {
        setOfficialSearch('');
        setSourceInput('');
        setSourceInputError('');
        setInspection(null);
        setInspectionRequest(null);
        setSelectedSkills({});
        setIsInspecting(false);
        setIsInstalling(false);
        setPendingSourceKey(null);
        setActionError(null);
        setActiveTab('recommended');
    }, []);

    const handleCloseDialog = useCallback(() => {
        resetTransientState();
        onClose();
    }, [onClose, resetTransientState]);

    useEffect(() => {
        if (!isOpen) {
            resetTransientState();
        }
    }, [isOpen, resetTransientState]);

    const fetchOfficialSkills = useCallback(async (useProxy: boolean) => {
        setFetchStatus('loading');
        setFetchError('');
        setIsFetchingWithProxy(useProxy);

        try {
            const skills = await invoke<OfficialSkill[]>('fetch_official_skills', { useProxy });
            setOfficialSkills(skills);
            setFetchStatus('success');
        } catch (error) {
            const message = String(error);
            setFetchError(message);
            setFetchStatus(
                message.includes('超时') || message.toLowerCase().includes('timeout')
                    ? 'timeout'
                    : 'error',
            );
        }
    }, []);

    useEffect(() => {
        if (isOpen && officialSkills.length === 0 && fetchStatus === 'idle') {
            fetchOfficialSkills(false);
        }
    }, [fetchOfficialSkills, fetchStatus, isOpen, officialSkills.length]);

    const handleRefreshOfficialSkills = useCallback(() => {
        void fetchOfficialSkills(isFetchingWithProxy);
    }, [fetchOfficialSkills, isFetchingWithProxy]);

    const handleOpenSkillsFolder = useCallback(async () => {
        setIsOpeningFolder(true);
        try {
            await invoke('open_skills_folder');
        } catch (error) {
            toast.error('打开 Skills 文件夹失败: ' + error);
        } finally {
            setIsOpeningFolder(false);
        }
    }, []);

    const beginInspection = useCallback(async (request: SourceRequest) => {
        setIsInspecting(true);
        setPendingSourceKey(buildSourceKey(request.source));
        setInspection(null);
        setInspectionRequest(request);
        setSelectedSkills({});
        setActionError(null);

        try {
            const result = await invoke<SkillArchiveInspection>('inspect_skill_archive_source', {
                source: request.source,
                dirs: request.dirs ?? null,
                useProxy: request.useProxy,
            });
            setInspection(result);
            setSelectedSkills(buildSelectionMap(result.skills));
        } catch (error) {
            const message = String(error);
            setActionError({ mode: 'inspect', request, message });
            toast.error('预览失败: ' + message);
        } finally {
            setIsInspecting(false);
            setPendingSourceKey(null);
        }
    }, []);

    const handleOfficialInspect = useCallback(
        async (skill: OfficialSkill, useProxy = false) => {
            const source = resolveOfficialSkillSource(skill);
            if (!source) {
                toast.error(`推荐项 ${skill.name} 缺少可用的下载源`);
                return;
            }

            await beginInspection({
                source,
                dirs: skill.dirs?.length ? skill.dirs : undefined,
                useProxy,
                title: skill.name,
                description: skill.description || undefined,
                sourceUrl: skill.source_url || undefined,
            });
        },
        [beginInspection],
    );

    const handleCustomInspect = useCallback(
        async (useProxy = false) => {
            let parsedSource;
            try {
                parsedSource = parseCustomSourceInput(sourceInput);
                setSourceInputError('');
            } catch (error) {
                const message = String(error);
                setSourceInputError(message);
                return;
            }

            await beginInspection({
                source: parsedSource.source,
                useProxy,
                title:
                    parsedSource.source.type === 'github'
                        ? `GitHub 仓库：${parsedSource.source.repo}`
                        : 'ZIP 链接',
                description:
                    parsedSource.source.type === 'github'
                        ? 'GitHub 仓库会通过 GitHub 提供的源码 ZIP 服务下载，再统一按压缩包流程识别和安装 Skills。'
                        : '将直接下载该 ZIP 并识别其中包含的 Skill 目录。',
                sourceUrl: parsedSource.sourceUrl,
            });
        },
        [beginInspection, sourceInput],
    );

    const selectedSkillList = useMemo(() => {
        if (!inspection) {
            return [] as SkillInstallPlanSkill[];
        }

        return inspection.skills.filter((skill) => selectedSkills[buildSkillSelectionKey(skill)]);
    }, [inspection, selectedSkills]);

    const selectedSkillCount = selectedSkillList.length;
    const allSelected = inspection ? selectedSkillCount === inspection.skills.length : false;
    const filteredOfficialSkills = useMemo(
        () => officialSkills.filter((skill) => matchesOfficialSkillSearch(skill, officialSearch)),
        [officialSkills, officialSearch],
    );

    const handleToggleSkill = useCallback((skill: SkillInstallPlanSkill, checked: boolean) => {
        const key = buildSkillSelectionKey(skill);
        setSelectedSkills((current) => ({
            ...current,
            [key]: checked,
        }));
    }, []);

    const handleSelectAll = useCallback(
        (checked: boolean) => {
            if (!inspection) {
                return;
            }

            setSelectedSkills(
                inspection.skills.reduce<Record<string, boolean>>((acc, skill) => {
                    acc[buildSkillSelectionKey(skill)] = checked;
                    return acc;
                }, {}),
            );
        },
        [inspection],
    );

    const runInstall = useCallback(
        async (request: SourceRequest) => {
            if (!inspection) {
                return;
            }

            const selections = selectedSkillList.map<SkillInstallRecipeDir>((skill) => ({
                from: skill.from,
                to: skill.to,
            }));

            if (selections.length === 0) {
                toast.error('请至少选择一个 Skill 进行安装');
                return;
            }

            setIsInstalling(true);
            setPendingSourceKey(buildSourceKey(request.source));
            setActionError(null);

            try {
                const result = await invoke<SkillArchiveInstallResult>('install_skill_archive_source', {
                    source: request.source,
                    selections,
                    useProxy: request.useProxy,
                });
                toast.success(`已安装 ${result.installed_skills.length} 个 Skill`);
                onSkillInstalled?.();
                handleCloseDialog();
            } catch (error) {
                const message = String(error);
                setActionError({ mode: 'install', request, message });
                toast.error('安装失败: ' + message);
            } finally {
                setIsInstalling(false);
                setPendingSourceKey(null);
            }
        },
        [handleCloseDialog, inspection, onSkillInstalled, selectedSkillList],
    );

    const handleRetryAction = useCallback(
        async (useProxy: boolean) => {
            if (!actionError) {
                return;
            }

            const retryRequest: SourceRequest = {
                ...actionError.request,
                useProxy,
            };

            if (actionError.mode === 'inspect') {
                await beginInspection(retryRequest);
                return;
            }

            await runInstall(retryRequest);
        },
        [actionError, beginInspection, runInstall],
    );

    const handleOpenSourceUrl = useCallback(async (url?: string | null) => {
        if (!url) {
            return;
        }

        try {
            await invoke('open_source_url', { url });
        } catch (error) {
            toast.error('打开来源链接失败: ' + error);
        }
    }, []);

    const handleBackToSourceList = useCallback(() => {
        setInspection(null);
        setInspectionRequest(null);
        setSelectedSkills({});
        setPendingSourceKey(null);
        setActionError(null);
        setIsInspecting(false);
        setIsInstalling(false);
    }, []);

    const sharedErrorPanel = actionError ? (
        <div className="rounded-lg border border-orange-200 bg-orange-50/80 dark:border-orange-900 dark:bg-orange-950/40 p-4 space-y-3">
            <div className="flex items-start gap-3">
                <ShieldAlert className="h-5 w-5 text-orange-600 dark:text-orange-300 mt-0.5" />
                <div className="space-y-1 min-w-0">
                    <div className="text-sm font-medium text-orange-900 dark:text-orange-100">
                        {actionError.mode === 'inspect' ? '预览失败' : '安装失败'}
                    </div>
                    <div className="text-sm text-orange-800 dark:text-orange-200 break-words">
                        {actionError.message}
                    </div>
                </div>
            </div>
            <div className="flex flex-wrap gap-2">
                <Button
                    variant="outline"
                    size="sm"
                    onClick={() => handleRetryAction(actionError.request.useProxy)}
                    disabled={isInspecting || isInstalling}
                >
                    {actionError.mode === 'inspect' ? '直接重试预览' : '直接重试安装'}
                </Button>
                {!actionError.request.useProxy && (
                    <Button
                        size="sm"
                        onClick={() => handleRetryAction(true)}
                        disabled={isInspecting || isInstalling}
                        className="gap-1"
                    >
                        <RefreshCw className="h-3.5 w-3.5" />
                        使用代理重试
                    </Button>
                )}
            </div>
        </div>
    ) : null;

    const remoteBusy = isInspecting && !inspection && activeTab === 'remote';
    const isFetchingOfficialSkills = fetchStatus === 'loading';
    const officialSearchActive = officialSearch.trim().length > 0;

    const recommendedContent = (() => {
        let body: React.ReactNode;

        if (fetchStatus === 'loading') {
            body = (
                <div className="flex flex-1 min-h-0 flex-col items-center justify-center py-10 text-muted-foreground gap-2">
                    <Loader2 className="h-6 w-6 animate-spin" />
                    <span className="text-sm">
                        {isFetchingWithProxy ? '使用代理加载推荐来源中...' : '加载推荐来源中...'}
                    </span>
                </div>
            );
        } else if (fetchStatus === 'error' || fetchStatus === 'timeout') {
            body = (
                <div className="flex flex-1 min-h-0 flex-col items-center justify-center py-8 text-center px-4 gap-3">
                    <ShieldAlert className="h-10 w-10 text-muted-foreground" />
                    <div>
                        <p className="text-sm font-medium">
                            {fetchStatus === 'timeout' ? '请求超时' : '加载失败'}
                        </p>
                        <p className="text-xs text-muted-foreground mt-1 break-words">{fetchError}</p>
                    </div>
                    <div className="flex flex-wrap gap-2 justify-center">
                        <Button variant="outline" size="sm" onClick={() => fetchOfficialSkills(false)}>
                            直接重试
                        </Button>
                        <Button size="sm" onClick={() => fetchOfficialSkills(true)} className="gap-1">
                            <RefreshCw className="h-3.5 w-3.5" />
                            使用代理
                        </Button>
                    </div>
                </div>
            );
        } else if (officialSkills.length === 0) {
            body = (
                <div className="flex flex-1 min-h-0 flex-col items-center justify-center py-8 text-center gap-3">
                    <p className="text-sm text-muted-foreground">暂无推荐来源</p>
                    <Button variant="outline" size="sm" onClick={handleRefreshOfficialSkills}>
                        刷新
                    </Button>
                </div>
            );
        } else if (filteredOfficialSkills.length === 0) {
            body = (
                <div className="flex flex-1 min-h-0 flex-col items-center justify-center py-8 text-center gap-3">
                    <p className="text-sm text-muted-foreground">没有匹配的推荐来源</p>
                    <p className="text-xs text-muted-foreground">试试更短的关键字，或点击刷新重新获取。</p>
                </div>
            );
        } else {
            body = (
                <div className="flex-1 min-h-0 overflow-y-auto pr-2">
                    <div className="space-y-3 pb-1">
                        {filteredOfficialSkills.map((skill) => {
                            const source = resolveOfficialSkillSource(skill);
                            const sourceKey = source ? buildSourceKey(source) : skill.id;
                            const isPending =
                                pendingSourceKey === sourceKey && (isInspecting || isInstalling);
                            return (
                                <Card key={skill.id} className="py-4">
                                    <CardHeader className="px-4 py-2 space-y-3">
                                        <div className="flex items-start justify-between gap-4">
                                            <div className="min-w-0 flex-1 space-y-2">
                                                <div className="flex flex-wrap items-center gap-2">
                                                    <CardTitle className="text-base">{skill.name}</CardTitle>
                                                    {skill.version && (
                                                        <Badge variant="outline">v{skill.version}</Badge>
                                                    )}
                                                    {source && (
                                                        <Badge variant="secondary">
                                                            {getSourceBadgeLabel(source)}
                                                        </Badge>
                                                    )}
                                                </div>
                                                <CardDescription>{skill.description}</CardDescription>
                                            </div>
                                            <div className="flex items-center gap-2 flex-shrink-0">
                                                {skill.source_url && (
                                                    <Button
                                                        variant="outline"
                                                        size="sm"
                                                        onClick={() => handleOpenSourceUrl(skill.source_url)}
                                                        className="gap-1"
                                                    >
                                                        <ExternalLink className="h-3.5 w-3.5" />
                                                        来源
                                                    </Button>
                                                )}
                                                <Button
                                                    size="sm"
                                                    onClick={() => handleOfficialInspect(skill)}
                                                    disabled={!source || isInspecting || isInstalling}
                                                    className="gap-1"
                                                >
                                                    {isPending ? (
                                                        <>
                                                            <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                                            识别中
                                                        </>
                                                    ) : (
                                                        <>
                                                            <Sparkles className="h-3.5 w-3.5" />
                                                            预览并选择
                                                        </>
                                                    )}
                                                </Button>
                                            </div>
                                        </div>
                                        {source && (
                                            <div className="text-xs text-muted-foreground break-all">
                                                {getSourceSummary(source)}
                                            </div>
                                        )}
                                    </CardHeader>
                                </Card>
                            );
                        })}
                    </div>
                </div>
            );
        }

        return (
            <div className="flex flex-1 min-h-0 flex-col gap-4">
                <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
                    <Input
                        value={officialSearch}
                        onChange={(event) => setOfficialSearch(event.target.value)}
                        placeholder="按名称、描述、ID 或来源筛选"
                        disabled={isFetchingOfficialSkills}
                        className="flex-1"
                    />
                    <Button
                        variant="outline"
                        size="sm"
                        onClick={handleRefreshOfficialSkills}
                        disabled={isFetchingOfficialSkills}
                        className="gap-1 sm:flex-shrink-0"
                    >
                        <RefreshCw
                            className={`h-3.5 w-3.5${isFetchingOfficialSkills ? ' animate-spin' : ''}`}
                        />
                        刷新
                    </Button>
                </div>

                {fetchStatus === 'success' && (
                    <div className="text-xs text-muted-foreground">
                        {officialSearchActive
                            ? `显示 ${filteredOfficialSkills.length} / ${officialSkills.length} 个推荐来源`
                            : `共 ${officialSkills.length} 个推荐来源`}
                    </div>
                )}

                {body}
            </div>
        );
    })();

    const remoteContent = (
        <div className="flex-1 min-h-0 space-y-4 overflow-y-auto pr-1">
            <div className="rounded-lg border p-4 space-y-4">
                <div className="space-y-1">
                    <div className="text-sm font-medium">从 GitHub 仓库或 ZIP 链接安装</div>
                    <p className="text-sm text-muted-foreground">
                        GitHub 仓库最终会通过 GitHub 提供的源码 ZIP 服务下载，因此后端统一按 ZIP 解压、识别和安装。
                    </p>
                </div>
                <div className="space-y-2">
                    <Input
                        value={sourceInput}
                        onChange={(event) => {
                            setSourceInput(event.target.value);
                            if (sourceInputError) {
                                setSourceInputError('');
                            }
                        }}
                        placeholder="例如：vercel-labs/agent-skills#main 或 https://example.com/skills.zip"
                        disabled={isInspecting || isInstalling}
                    />
                    {sourceInputError && (
                        <p className="text-xs text-destructive">{sourceInputError}</p>
                    )}
                </div>
                <div className="text-xs text-muted-foreground space-y-1">
                    <div>支持格式：</div>
                    <div className="font-mono">owner/repo#ref</div>
                    <div className="font-mono">https://github.com/owner/repo/tree/main</div>
                    <div className="font-mono">https://example.com/shared-skills.zip</div>
                </div>
                <div className="flex flex-wrap gap-2">
                    <Button
                        onClick={() => handleCustomInspect(false)}
                        disabled={isInspecting || isInstalling}
                        className="gap-1"
                    >
                        {remoteBusy ? (
                            <>
                                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                识别中
                            </>
                        ) : (
                            <>
                                <Sparkles className="h-3.5 w-3.5" />
                                识别 Skills
                            </>
                        )}
                    </Button>
                </div>
            </div>
        </div>
    );

    const manualContent = (
        <div className="flex-1 min-h-0 space-y-4 overflow-y-auto pr-1">
            <ol className="space-y-4">
                {installSteps.map((step, index) => (
                    <li key={step.title} className="flex gap-3">
                        <span className="flex-shrink-0 w-6 h-6 rounded-full bg-primary text-primary-foreground text-sm font-medium flex items-center justify-center">
                            {index + 1}
                        </span>
                        <div className="flex-1 space-y-2">
                            <h4 className="font-medium text-sm">{step.title}</h4>
                            <p className="text-sm text-muted-foreground">{step.description}</p>
                            {index === 0 && (
                                <Button
                                    variant="outline"
                                    size="sm"
                                    onClick={handleOpenSkillsFolder}
                                    disabled={isOpeningFolder}
                                    className="gap-1.5"
                                >
                                    <FolderOpen className="h-4 w-4" />
                                    {isOpeningFolder ? '打开中...' : '打开 Skills 文件夹'}
                                </Button>
                            )}
                        </div>
                    </li>
                ))}
            </ol>
        </div>
    );

    const previewContent = inspection && inspectionRequest ? (
        <div className="flex-1 min-h-0 flex flex-col gap-4">
            <div className="flex flex-wrap items-start justify-between gap-3">
                <div className="min-w-0 space-y-2">
                    <Button
                        variant="ghost"
                        size="sm"
                        onClick={handleBackToSourceList}
                        disabled={isInspecting || isInstalling}
                        className="-ml-2 w-fit gap-1"
                    >
                        <ArrowLeft className="h-4 w-4" />
                        返回来源列表
                    </Button>
                    <div className="space-y-1">
                        <div className="flex flex-wrap items-center gap-2">
                            <h3 className="text-lg font-semibold">{inspectionRequest.title}</h3>
                            <Badge variant="secondary">{getSourceBadgeLabel(inspection.source)}</Badge>
                            <Badge variant="outline">发现 {inspection.skills.length} 个 Skills</Badge>
                            {inspectionRequest.useProxy && <Badge variant="outline">已使用代理</Badge>}
                        </div>
                        <p className="text-sm text-muted-foreground break-all">{inspection.source_label}</p>
                        {inspectionRequest.description && (
                            <p className="text-sm text-muted-foreground">{inspectionRequest.description}</p>
                        )}
                    </div>
                </div>
                {inspectionRequest.sourceUrl && (
                    <Button
                        variant="outline"
                        size="sm"
                        onClick={() => handleOpenSourceUrl(inspectionRequest.sourceUrl)}
                        className="gap-1"
                    >
                        <ExternalLink className="h-3.5 w-3.5" />
                        查看来源
                    </Button>
                )}
            </div>

            {sharedErrorPanel}

            <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border p-3 bg-muted/30">
                <div className="text-sm text-muted-foreground">
                    已选择 <span className="font-medium text-foreground">{selectedSkillCount}</span> / {inspection.skills.length}
                </div>
                <div className="flex items-center gap-2">
                    <div className="flex items-center gap-2 pr-2">
                        <Checkbox
                            checked={allSelected}
                            onCheckedChange={(checked) => handleSelectAll(Boolean(checked))}
                            disabled={isInspecting || isInstalling}
                        />
                        <span className="text-sm text-muted-foreground">全部安装</span>
                    </div>
                    <Button
                        variant="outline"
                        size="sm"
                        onClick={() => handleSelectAll(true)}
                        disabled={allSelected || isInspecting || isInstalling}
                    >
                        全选
                    </Button>
                    <Button
                        variant="outline"
                        size="sm"
                        onClick={() => handleSelectAll(false)}
                        disabled={selectedSkillCount === 0 || isInspecting || isInstalling}
                    >
                        清空
                    </Button>
                </div>
            </div>

            <div className="flex-1 min-h-0 overflow-y-auto rounded-md border">
                <div className="space-y-3 p-4">
                    {inspection.skills.map((skill) => {
                        const skillKey = buildSkillSelectionKey(skill);
                        const checked = selectedSkills[skillKey] ?? false;
                        return (
                            <div
                                key={skillKey}
                                className="rounded-lg border p-4 space-y-3 cursor-pointer hover:bg-muted/40 transition-colors"
                                onClick={() => handleToggleSkill(skill, !checked)}
                            >
                                <div className="flex items-start gap-3">
                                    <Checkbox
                                        checked={checked}
                                        onCheckedChange={(value) => handleToggleSkill(skill, Boolean(value))}
                                        onClick={(event) => event.stopPropagation()}
                                        className="mt-1"
                                    />
                                    <div className="min-w-0 flex-1 space-y-3">
                                        <div className="flex flex-wrap items-center gap-2">
                                            <div className="font-medium">{skill.display_name}</div>
                                            <Badge variant="outline">~/.agents/skills/{skill.to}</Badge>
                                            {skill.will_replace && (
                                                <Badge variant="secondary" className="text-destructive">
                                                    将覆盖已有目录
                                                </Badge>
                                            )}
                                        </div>

                                        {(skill.preview || skill.metadata.description) && (
                                            <p className="text-sm text-muted-foreground whitespace-pre-wrap break-words">
                                                {skill.preview || skill.metadata.description}
                                            </p>
                                        )}

                                        <div className="grid gap-1 text-xs text-muted-foreground">
                                            <div>
                                                来源目录：<code className="rounded bg-muted px-1 py-0.5">{skill.from}</code>
                                            </div>
                                            <div>
                                                入口文件：
                                                <code className="rounded bg-muted px-1 py-0.5 ml-1">
                                                    {skill.normalized_entry_file}
                                                </code>
                                                {skill.detected_entry_file !== skill.normalized_entry_file && (
                                                    <span className="ml-2">（检测到 {skill.detected_entry_file}，安装时会归一化）</span>
                                                )}
                                            </div>
                                        </div>

                                        {skill.metadata.tags.length > 0 && (
                                            <div className="flex flex-wrap gap-2">
                                                {skill.metadata.tags.slice(0, 8).map((tag) => (
                                                    <Badge key={tag} variant="outline">
                                                        {tag}
                                                    </Badge>
                                                ))}
                                            </div>
                                        )}
                                    </div>
                                </div>
                            </div>
                        );
                    })}
                </div>
            </div>

        </div>
    ) : null;

    return (
        <Dialog open={isOpen} onOpenChange={(open) => !open && handleCloseDialog()}>
            <DialogContent className="!w-[50vw] !max-w-[50vw] max-h-[85vh] overflow-hidden flex flex-col">
                <DialogHeader>
                    <DialogTitle className="flex items-center gap-2">
                        <BookOpen className="h-5 w-5" />
                        Skills 安装
                    </DialogTitle>
                    <DialogDescription>
                        支持推荐来源、GitHub 仓库或 ZIP 链接。预览后可批量勾选要安装的 Skills，并在失败时切换代理重试。
                    </DialogDescription>
                </DialogHeader>

                {!inspection && sharedErrorPanel}

                {previewContent ?? (
                    <Tabs
                        value={activeTab}
                        onValueChange={(value) => setActiveTab(value as InstallTab)}
                        className="flex-1 min-h-0 overflow-hidden flex flex-col"
                    >
                        <TabsList className="w-full justify-start">
                            <TabsTrigger value="recommended" className="flex-1">
                                推荐来源
                            </TabsTrigger>
                            <TabsTrigger value="remote" className="flex-1">
                                仓库 / ZIP
                            </TabsTrigger>
                            <TabsTrigger value="manual" className="flex-1">
                                手动安装
                            </TabsTrigger>
                        </TabsList>

                        <TabsContent
                            value="recommended"
                            className="mt-4 flex flex-1 min-h-0 flex-col overflow-hidden"
                        >
                            {recommendedContent}
                        </TabsContent>

                        <TabsContent
                            value="remote"
                            className="mt-4 flex flex-1 min-h-0 flex-col overflow-hidden"
                        >
                            {remoteContent}
                        </TabsContent>

                        <TabsContent
                            value="manual"
                            className="mt-4 flex flex-1 min-h-0 flex-col overflow-hidden"
                        >
                            {manualContent}
                        </TabsContent>
                    </Tabs>
                )}

                <Separator className="mt-4" />

                <DialogFooter className="mt-4 flex-shrink-0">
                    {inspection && inspectionRequest ? (
                        <>
                            <Button
                                variant="outline"
                                onClick={handleBackToSourceList}
                                disabled={isInspecting || isInstalling}
                            >
                                返回
                            </Button>
                            <Button
                                onClick={() => runInstall(inspectionRequest)}
                                disabled={selectedSkillCount === 0 || isInspecting || isInstalling}
                                className="gap-1"
                            >
                                {isInstalling ? (
                                    <>
                                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                        安装中
                                    </>
                                ) : (
                                    <>
                                        <Download className="h-3.5 w-3.5" />
                                        安装已选 {selectedSkillCount > 0 ? `(${selectedSkillCount})` : ''}
                                    </>
                                )}
                            </Button>
                        </>
                    ) : (
                        <Button onClick={handleCloseDialog}>关闭</Button>
                    )}
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
};

export default SkillInstallGuideDialog;
