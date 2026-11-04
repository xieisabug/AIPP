import { useMemo, useRef } from 'react';
import { Message } from '../data/Conversation';
import { CodeArtifact } from '../components/chat-sidebar/types';
import { parseCodeBlockMeta } from '../react-markdown/remarkCodeBlockMeta';

// Regex to match markdown code blocks with optional meta info
// Matches ```lang meta\n...``` where meta can contain title="..." or other attributes
// Group 1: language, Group 2: meta info (optional), Group 3: code content
const CODE_BLOCK_REGEX = /```(\w*)([^\n]*)\n([\s\S]*?)```/g;


const ARTIFACT_LIST_LANGUAGES = new Set([
    'vue',
    'react',
    'jsx',
    'tsx',
    'html',
    'markdown',
    'md',
    'mermaid',
    'drawio',
]);

interface UseArtifactExtractorOptions {
    messages: Message[];
    activeStreamingMessageIds?: ReadonlySet<number>;
}

interface UseArtifactExtractorReturn {
    artifacts: CodeArtifact[];
}

function extractMessageArtifacts(message: Message, messageIdx: number): CodeArtifact[] {
    if (!message.content) {
        return [];
    }

    const result: CodeArtifact[] = [];
    let match: RegExpExecArray | null;
    let blockIndex = 0;
    CODE_BLOCK_REGEX.lastIndex = 0;

    while ((match = CODE_BLOCK_REGEX.exec(message.content)) !== null) {
        const language = match[1] || 'text';
        const meta = match[2] || '';
        const code = match[3].trim();

        const normalizedLanguage = language.toLowerCase();
        if (!ARTIFACT_LIST_LANGUAGES.has(normalizedLanguage)) continue;
        if (code.length < 20) continue;

        blockIndex++;
        const metaInfo = meta.trim() ? parseCodeBlockMeta(meta) : {};
        const metaTitle = metaInfo.title;
        const displayTitle = metaTitle || `消息${messageIdx + 1} - 代码块${blockIndex}`;

        result.push({
            id: `artifact-${message.id}-${blockIndex}`,
            language,
            code,
            messageId: message.id,
            messageIndex: messageIdx + 1,
            blockIndex,
            metaTitle,
            title: displayTitle,
        });
    }

    return result;
}

function sameArtifacts(previous: CodeArtifact[] | undefined, next: CodeArtifact[]): boolean {
    if (!previous || previous.length !== next.length) {
        return false;
    }
    return previous.every((artifact, index) => {
        const other = next[index];
        return artifact.id === other.id
            && artifact.code === other.code
            && artifact.language === other.language
            && artifact.title === other.title;
    });
}

export function useArtifactExtractor({
    messages,
    activeStreamingMessageIds,
}: UseArtifactExtractorOptions): UseArtifactExtractorReturn {
    const artifactCacheRef = useRef(new Map<number, CodeArtifact[]>());
    const artifactsRef = useRef<CodeArtifact[]>([]);

    const artifacts = useMemo(() => {
        const result: CodeArtifact[] = [];
        const responseMessages = messages.filter(
            (message) => message.message_type === 'assistant' || message.message_type === 'response',
        );

        responseMessages.forEach((message, messageIdx) => {
            if (activeStreamingMessageIds?.has(message.id)) {
                result.push(...(artifactCacheRef.current.get(message.id) ?? []));
                return;
            }

            const extracted = extractMessageArtifacts(message, messageIdx);
            const cached = artifactCacheRef.current.get(message.id);
            if (!sameArtifacts(cached, extracted)) {
                artifactCacheRef.current.set(message.id, extracted);
            }
            result.push(...(artifactCacheRef.current.get(message.id) ?? extracted));
        });

        const visibleIds = new Set(responseMessages.map((message) => message.id));
        for (const messageId of artifactCacheRef.current.keys()) {
            if (!visibleIds.has(messageId)) {
                artifactCacheRef.current.delete(messageId);
            }
        }

        const previous = artifactsRef.current;
        if (
            previous.length === result.length
            && previous.every((artifact, index) => {
                const next = result[index];
                return artifact.id === next.id
                    && artifact.code === next.code
                    && artifact.language === next.language
                    && artifact.title === next.title;
            })
        ) {
            return previous;
        }
        artifactsRef.current = result;
        return result;
    }, [activeStreamingMessageIds, messages]);

    return { artifacts };
}
