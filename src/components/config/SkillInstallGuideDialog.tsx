import React, { useEffect, useState } from 'react';
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from '../ui/dialog';
import { Button } from '../ui/button';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../ui/tabs';
import { Card, CardDescription, CardHeader, CardTitle } from '../ui/card';
import { BookOpen, FolderOpen, ExternalLink, Download, Loader2, RefreshCw, ShieldAlert } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';

interface InstallStep {
    title: string;
    description: string | React.ReactNode;
}

interface SkillInstallGuideDialogProps {
    isOpen: boolean;
    onClose: () => void;
    onSkillInstalled?: () => void;
}

interface OfficialSkill {
    id: string;
    name: string;
    description: string;
    version: string;
    download_url: string;
    source_url: string;
}

type FetchStatus = 'idle' | 'loading' | 'success' | 'error' | 'timeout';

// 安装步骤（占位内容，可根据实际需求修改）
const installSteps: InstallStep[] = [
    {
        title: '步骤一：打开AIPP Skills文件夹',
        description: '点击”打开Skills文件夹“按钮，或直接访问应用数据目录下的skills文件夹'
    },
    {
        title: '步骤二：下载Skill文件夹',
        description: '从可信任的第三方来源下载所需的skill文件夹，并确认其中包含 SKILLS.md 文件'
    },
    {
        title: '步骤三：安装Skill',
        description: '将下载的skill文件夹复制到Skills文件夹中'
    },
    {
        title: '步骤四：扫描并启用',
        description: '点击"扫描Skills"按钮，在列表中找到新安装的Skill并启用'
    }
];

const SkillInstallGuideDialog: React.FC<SkillInstallGuideDialogProps> = ({
    isOpen,
    onClose,
    onSkillInstalled
}) => {
    const [activeTab, setActiveTab] = useState<'manual' | 'recommended'>('recommended');
    const [isOpeningFolder, setIsOpeningFolder] = useState(false);
    const [officialSkills, setOfficialSkills] = useState<OfficialSkill[]>([]);
    const [fetchStatus, setFetchStatus] = useState<FetchStatus>('idle');
    const [fetchError, setFetchError] = useState<string>('');
    const [isUsingProxy, setIsUsingProxy] = useState(false);
    const [installingSkillId, setInstallingSkillId] = useState<string | null>(null);

    // Fetch official skills when dialog opens
    useEffect(() => {
        if (isOpen && officialSkills.length === 0 && fetchStatus === 'idle') {
            fetchOfficialSkills(false);
        }
    }, [isOpen]);

    const fetchOfficialSkills = async (useProxy: boolean) => {
        setFetchStatus('loading');
        setIsUsingProxy(useProxy);
        setFetchError('');
        try {
            const skills = await invoke<OfficialSkill[]>('fetch_official_skills', { useProxy });
            setOfficialSkills(skills);
            setFetchStatus('success');
        } catch (error) {
            const errorMsg = String(error);
            setFetchError(errorMsg);
            console.error('Failed to fetch official skills:', error);
            // Check if it's a timeout
            if (errorMsg.includes('超时') || errorMsg.includes('timeout')) {
                setFetchStatus('timeout');
            } else {
                setFetchStatus('error');
            }
        }
    };

    const handleOpenSkillsFolder = async () => {
        setIsOpeningFolder(true);
        try {
            await invoke('open_skills_folder');
        } catch (error) {
            console.error('Failed to open skills folder:', error);
        } finally {
            setIsOpeningFolder(false);
        }
    };

    const handleInstall = async (skill: OfficialSkill) => {
        setInstallingSkillId(skill.id);
        try {
            await invoke('install_official_skill', { downloadUrl: skill.download_url });
            // Trigger skill scan to refresh the list
            await invoke('scan_skills');
            // Call the callback if provided
            onSkillInstalled?.();
            // Close the dialog
            onClose();
        } catch (error) {
            console.error('Failed to install skill:', error);
            alert(`安装失败: ${error}`);
        } finally {
            setInstallingSkillId(null);
        }
    };

    const handleSource = async (skill: OfficialSkill) => {
        try {
            await invoke('open_source_url', { url: skill.source_url });
        } catch (error) {
            console.error('Failed to open source URL:', error);
        }
    };

    const handleRetry = () => {
        // Retry with proxy if it was a timeout
        fetchOfficialSkills(true);
    };

    const handleRetryWithoutProxy = () => {
        fetchOfficialSkills(false);
    };

    return (
        <Dialog open={isOpen} onOpenChange={onClose}>
            <DialogContent className="max-w-2xl max-h-[80vh] overflow-hidden flex flex-col">
                <DialogHeader>
                    <DialogTitle className="flex items-center gap-2">
                        <BookOpen className="h-5 w-5" />
                        Skills安装
                    </DialogTitle>
                    <DialogDescription>
                        选择安装方式：手动安装自定义 Skills，或从官方推荐中安装
                    </DialogDescription>
                </DialogHeader>

                <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as 'manual' | 'recommended')} className="flex-1 overflow-hidden flex flex-col">
                    <TabsList className="w-full justify-start">
                        <TabsTrigger value="recommended" className="flex-1">官方推荐</TabsTrigger>
                        <TabsTrigger value="manual" className="flex-1">手动安装</TabsTrigger>
                    </TabsList>

                    <TabsContent value="manual" className="flex-1 overflow-y-auto mt-4">
                        <ol className="space-y-4">
                            {installSteps.map((step, index) => (
                                <li key={index} className="flex gap-3">
                                    <span className="flex-shrink-0 w-6 h-6 rounded-full bg-primary text-primary-foreground text-sm font-medium flex items-center justify-center">
                                        {index + 1}
                                    </span>
                                    <div className="flex-1">
                                        <h4 className="font-medium text-sm">{step.title}</h4>
                                        {index === 0 ? (
                                            <div className="flexgap-2 mt-2">
                                                <p className="text-sm text-muted-foreground">{step.description}</p>
                                                <Button
                                                    variant="outline"
                                                    size="sm"
                                                    onClick={handleOpenSkillsFolder}
                                                    disabled={isOpeningFolder}
                                                    className="gap-1.5"
                                                >
                                                    <FolderOpen className="h-4 w-4" />
                                                    {isOpeningFolder ? '打开中...' : '打开Skills文件夹'}
                                                </Button>
                                            </div>
                                        ) : (
                                            <p className="text-sm text-muted-foreground mt-1">{step.description}</p>
                                        )}
                                    </div>
                                </li>
                            ))}
                        </ol>
                    </TabsContent>

                    <TabsContent value="recommended" className="flex-1 overflow-y-auto mt-4">
                        {fetchStatus === 'loading' ? (
                            <div className="flex flex-col items-center justify-center py-8">
                                <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
                                <span className="ml-2 text-sm text-muted-foreground">
                                    {isUsingProxy ? '使用代理加载中...' : '加载中...'}
                                </span>
                            </div>
                        ) : fetchStatus === 'timeout' ? (
                            <div className="flex flex-col items-center justify-center py-8 text-center px-4">
                                <ShieldAlert className="h-10 w-10 text-muted-foreground mb-3" />
                                <p className="text-sm text-muted-foreground mb-1">请求超时</p>
                                <p className="text-xs text-muted-foreground mb-4">{fetchError}</p>
                                <Button
                                    variant="default"
                                    size="sm"
                                    onClick={handleRetry}
                                    className="gap-1"
                                >
                                    <RefreshCw className="h-3.5 w-3.5" />
                                    使用代理重试
                                </Button>
                            </div>
                        ) : fetchStatus === 'error' ? (
                            <div className="flex flex-col items-center justify-center py-8 text-center px-4">
                                <ShieldAlert className="h-10 w-10 text-muted-foreground mb-3" />
                                <p className="text-sm text-muted-foreground mb-1">加载失败</p>
                                <p className="text-xs text-muted-foreground mb-4">{fetchError}</p>
                                <div className="flex gap-2">
                                    <Button
                                        variant="outline"
                                        size="sm"
                                        onClick={handleRetryWithoutProxy}
                                    >
                                        直接重试
                                    </Button>
                                    <Button
                                        variant="default"
                                        size="sm"
                                        onClick={handleRetry}
                                        className="gap-1"
                                    >
                                        <RefreshCw className="h-3.5 w-3.5" />
                                        使用代理
                                    </Button>
                                </div>
                            </div>
                        ) : officialSkills.length === 0 ? (
                            <div className="flex flex-col items-center justify-center py-8 text-center">
                                <p className="text-sm text-muted-foreground">暂无官方技能</p>
                                <Button
                                    variant="outline"
                                    size="sm"
                                    onClick={() => fetchOfficialSkills(false)}
                                    className="mt-2"
                                >
                                    重试
                                </Button>
                            </div>
                        ) : (
                            <div className="space-y-3">
                                {officialSkills.map((skill) => (
                                    <Card key={skill.id} className="py-4">
                                        <CardHeader className="px-4 py-2">
                                            <div className="flex items-start justify-between gap-4">
                                                <div className="flex-1 min-w-0">
                                                    <CardTitle className="text-base">{skill.name}</CardTitle>
                                                    <CardDescription className="mt-1">{skill.description}</CardDescription>
                                                </div>
                                                <div className="flex items-center gap-2 flex-shrink-0">
                                                    <Button
                                                        variant="outline"
                                                        size="sm"
                                                        onClick={() => handleSource(skill)}
                                                        className="gap-1"
                                                    >
                                                        <ExternalLink className="h-3.5 w-3.5" />
                                                        来源
                                                    </Button>
                                                    <Button
                                                        size="sm"
                                                        onClick={() => handleInstall(skill)}
                                                        disabled={installingSkillId === skill.id}
                                                        className="gap-1"
                                                    >
                                                        {installingSkillId === skill.id ? (
                                                            <>
                                                                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                                                安装中
                                                            </>
                                                        ) : (
                                                            <>
                                                                <Download className="h-3.5 w-3.5" />
                                                                安装
                                                            </>
                                                        )}
                                                    </Button>
                                                </div>
                                            </div>
                                            <div className="mt-2 flex items-center gap-2 text-xs text-muted-foreground">
                                                <span className="px-2 py-0.5 rounded-md bg-muted">v{skill.version}</span>
                                            </div>
                                        </CardHeader>
                                    </Card>
                                ))}
                            </div>
                        )}
                    </TabsContent>
                </Tabs>

                <DialogFooter className="mt-4">
                    <Button onClick={onClose}>关闭</Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
};

export default SkillInstallGuideDialog;
