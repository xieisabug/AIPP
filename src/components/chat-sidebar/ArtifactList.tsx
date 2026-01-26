import React, { useCallback } from 'react';
import { Code, FileCode, FileJson, FileText, Copy } from 'lucide-react';
import { CodeArtifact } from './types';
import { cn } from '@/utils/utils';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';

interface ArtifactListProps {
    artifacts: CodeArtifact[];
    className?: string;
    onArtifactClick?: (artifact: CodeArtifact) => void;
}

const getLanguageIcon = (language: string) => {
    const lang = language.toLowerCase();
    if (['json', 'jsonc'].includes(lang)) {
        return <FileJson className="h-4 w-4 flex-shrink-0" />;
    }
    if (['text', 'txt', 'md', 'markdown'].includes(lang)) {
        return <FileText className="h-4 w-4 flex-shrink-0" />;
    }
    if (['js', 'javascript', 'ts', 'typescript', 'jsx', 'tsx'].includes(lang)) {
        return <FileCode className="h-4 w-4 text-yellow-500 flex-shrink-0" />;
    }
    return <Code className="h-4 w-4 flex-shrink-0" />;
};

const getLanguageColor = (language: string): string => {
    const lang = language.toLowerCase();
    const colors: Record<string, string> = {
        javascript: 'text-yellow-500',
        js: 'text-yellow-500',
        typescript: 'text-blue-500',
        ts: 'text-blue-500',
        tsx: 'text-blue-500',
        jsx: 'text-yellow-500',
        python: 'text-green-500',
        py: 'text-green-500',
        rust: 'text-orange-500',
        rs: 'text-orange-500',
        go: 'text-cyan-500',
        java: 'text-red-500',
        cpp: 'text-purple-500',
        c: 'text-purple-500',
        html: 'text-orange-400',
        css: 'text-blue-400',
        json: 'text-gray-500',
        sql: 'text-pink-500',
        bash: 'text-green-400',
        shell: 'text-green-400',
        sh: 'text-green-400',
    };
    return colors[lang] || 'text-muted-foreground';
};

const ArtifactList: React.FC<ArtifactListProps> = ({ artifacts, className, onArtifactClick }) => {
    const handleCopy = useCallback(async (code: string, e: React.MouseEvent) => {
        e.stopPropagation();
        try {
            await writeText(code);
            toast.success('已复制到剪贴板');
        } catch (err) {
            toast.error('复制失败');
        }
    }, []);

    if (artifacts.length === 0) {
        return (
            <div className={cn("p-3 text-sm text-muted-foreground text-center", className)}>
                暂无代码块
            </div>
        );
    }

    return (
        <div className={cn("flex flex-col gap-1 p-2 max-h-56 overflow-y-auto", className)}>
            {artifacts.map((artifact) => (
                <div
                    key={artifact.id}
                    className={cn(
                        "flex items-center gap-2 p-2 rounded-md transition-colors",
                        "hover:bg-muted/50 cursor-pointer group"
                    )}
                    onClick={() => onArtifactClick?.(artifact)}
                >
                    <div className={cn("flex-shrink-0", getLanguageColor(artifact.language))}>
                        {getLanguageIcon(artifact.language)}
                    </div>
                    <div className="flex-1 min-w-0">
                        <p className="text-sm truncate">{artifact.title}</p>
                        <p className="text-xs text-muted-foreground">{artifact.language}</p>
                    </div>
                    <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                        <Button
                            variant="ghost"
                            size="icon"
                            className="h-6 w-6"
                            onClick={(e) => handleCopy(artifact.code, e)}
                        >
                            <Copy className="h-3 w-3" />
                        </Button>
                    </div>
                </div>
            ))}
        </div>
    );
};

export default React.memo(ArtifactList);
