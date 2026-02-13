import React, { useEffect, useMemo, useState } from 'react';
import { useTheme } from '@/hooks/useTheme';
import { useCodeTheme } from '@/hooks/useCodeTheme';
import { useRustHighlight } from '@/hooks/highlight/useRustHighlight';

interface ArtifactPreviewCodeBlockProps {
    language: string;
    children: React.ReactNode;
    className?: string;
}

const ArtifactPreviewCodeBlock: React.FC<ArtifactPreviewCodeBlockProps> = ({
    language,
    children,
    className = '',
}) => {
    const code = useMemo(() => (typeof children === 'string' ? children : String(children)), [children]);
    const { resolvedTheme } = useTheme();
    const { currentTheme } = useCodeTheme();
    const rustHighlight = useRustHighlight();
    const [html, setHtml] = useState<string>('');

    useEffect(() => {
        let cancelled = false;
        (async () => {
            try {
                const result = await rustHighlight(language, code, resolvedTheme === 'dark', currentTheme);
                if (!cancelled) setHtml(result);
            } catch (e) {
                console.warn('[ArtifactPreviewCodeBlock] highlight failed, fallback to plain text', e);
                if (!cancelled) setHtml('');
            }
        })();
        return () => {
            cancelled = true;
        };
    }, [language, code, resolvedTheme, currentTheme, rustHighlight]);

    return (
        <div className={`w-full min-w-0 ${className}`}>
            {html ? (
                <div className="text-sm font-mono" dangerouslySetInnerHTML={{ __html: html }} />
            ) : (
                <pre className="text-sm font-mono whitespace-pre-wrap">
                    <code>{code}</code>
                </pre>
            )}
        </div>
    );
};

export default React.memo(ArtifactPreviewCodeBlock);
