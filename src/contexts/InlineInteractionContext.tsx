import { createContext, useContext, type ReactNode } from "react";
import {
    AskUserQuestionRequest,
    PreviewFileRequest,
} from "@/components/InlineInteractionCards";

interface AskUserQuestionState {
    request: AskUserQuestionRequest | null;
    isOpen: boolean;
    viewMode: "questionnaire" | "summary";
    completedAnswers: Record<string, string> | null;
    readOnly: boolean;
    callId: number | null;
    onSubmit: (requestId: string, answers: Record<string, string>) => void;
    onCancel: (requestId: string) => void;
}

interface PreviewFileState {
    request: PreviewFileRequest | null;
    isOpen: boolean;
    callId: number | null;
    onOpenChange: (open: boolean) => void;
}

interface InlineInteractionContextType {
    askUserQuestion: AskUserQuestionState | null;
    previewFile: PreviewFileState | null;
}

const InlineInteractionContext = createContext<InlineInteractionContextType | null>(null);

export interface InlineInteractionProviderProps {
    children: ReactNode;
    askUserQuestion: AskUserQuestionState | null;
    previewFile: PreviewFileState | null;
}

export function InlineInteractionProvider({
    children,
    askUserQuestion,
    previewFile,
}: InlineInteractionProviderProps) {
    return (
        <InlineInteractionContext.Provider value={{ askUserQuestion, previewFile }}>
            {children}
        </InlineInteractionContext.Provider>
    );
}

export function useInlineInteraction() {
    const context = useContext(InlineInteractionContext);
    if (!context) {
        throw new Error("useInlineInteraction must be used within an InlineInteractionProvider");
    }
    return context;
}
