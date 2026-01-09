import React from 'react';
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from '../ui/dialog';
import { Button } from '../ui/button';
import { BookOpen } from 'lucide-react';

interface InstallStep {
    title: string;
    description: string;
}

interface SkillInstallGuideDialogProps {
    isOpen: boolean;
    onClose: () => void;
}

// 安装步骤（占位内容，可根据实际需求修改）
const installSteps: InstallStep[] = [
    {
        title: '步骤一：打开Skills文件夹',
        description: '点击"打开Skills文件夹"按钮，或直接访问应用数据目录下的skills文件夹'
    },
    {
        title: '步骤二：下载Skill文件',
        description: '从官方仓库或第三方来源下载所需的.skill文件'
    },
    {
        title: '步骤三：安装Skill',
        description: '将下载的.skill文件复制到Skills文件夹中'
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
    return (
        <Dialog open={isOpen} onOpenChange={onClose}>
            <DialogContent className="max-w-md">
                <DialogHeader>
                    <DialogTitle className="flex items-center gap-2">
                        <BookOpen className="h-5 w-5" />
                        Skills安装指南
                    </DialogTitle>
                    <DialogDescription>
                        按照以下步骤安装和配置Skills
                    </DialogDescription>
                </DialogHeader>

                <ol className="space-y-4 my-4">
                    {installSteps.map((step, index) => (
                        <li key={index} className="flex gap-3">
                            <span className="flex-shrink-0 w-6 h-6 rounded-full bg-primary text-primary-foreground text-sm font-medium flex items-center justify-center">
                                {index + 1}
                            </span>
                            <div className="flex-1">
                                <h4 className="font-medium text-sm">{step.title}</h4>
                                <p className="text-sm text-muted-foreground mt-1">{step.description}</p>
                            </div>
                        </li>
                    ))}
                </ol>

                <DialogFooter>
                    <Button onClick={onClose}>知道了</Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
};

export default SkillInstallGuideDialog;
