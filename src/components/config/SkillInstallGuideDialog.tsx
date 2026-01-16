import React, { useState } from 'react';
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
import { BookOpen, FolderOpen, ExternalLink, Download } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';

interface InstallStep {
    title: string;
    description: string | React.ReactNode;
}

interface SkillInstallGuideDialogProps {
    isOpen: boolean;
    onClose: () => void;
}

interface RecommendedSkill {
    id: string;
    name: string;
    description: string;
    version: string;
}

// 官方推荐技能 Mock 数据
const recommendedSkills: RecommendedSkill[] = [
    {
        id: 'frontend-design',
        name: '前端设计专家',
        description: '创建独特、生产级的前端界面，具有高设计质量。支持 React、Tailwind CSS 和 shadcn/ui。',
        version: '1.0.0'
    },
    {
        id: 'doc-coauthoring',
        name: '文档协作助手',
        description: '引导用户完成结构化的文档协作工作流程，适用于编写文档、提案、技术规范等。',
        version: '1.2.0'
    },
    {
        id: 'pdf-tools',
        name: 'PDF处理工具',
        description: '全面的 PDF 操作工具包，支持提取文本和表格、创建新 PDF、合并/拆分文档以及处理表单。',
        version: '2.0.1'
    },
    {
        id: 'spreadsheet',
        name: '电子表格专家',
        description: '综合电子表格创建、编辑和分析，支持公式、格式化、数据分析和可视化。',
        version: '1.5.0'
    }
];

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
    onClose
}) => {
    const [activeTab, setActiveTab] = useState<'manual' | 'recommended'>('manual');
    const [isOpeningFolder, setIsOpeningFolder] = useState(false);

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

    const handleInstall = (skillId: string) => {
        // TODO: 实现安装功能
        console.log('Install skill:', skillId);
    };

    const handleSource = (skillId: string) => {
        // TODO: 实现来源功能
        console.log('View source:', skillId);
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
                        <div className="space-y-3">
                            {recommendedSkills.map((skill) => (
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
                                                    onClick={() => handleSource(skill.id)}
                                                    className="gap-1"
                                                >
                                                    <ExternalLink className="h-3.5 w-3.5" />
                                                    来源
                                                </Button>
                                                <Button
                                                    size="sm"
                                                    onClick={() => handleInstall(skill.id)}
                                                    className="gap-1"
                                                >
                                                    <Download className="h-3.5 w-3.5" />
                                                    安装
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
