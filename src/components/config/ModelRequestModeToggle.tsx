import { ChevronDown } from "lucide-react";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuLabel,
    DropdownMenuRadioGroup,
    DropdownMenuRadioItem,
    DropdownMenuSeparator,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { requestModeLabels } from "./llmModelTypes";

interface Props {
    requestMode: string;
    availableModes?: string[];
    onToggle: (mode: string) => void;
}

const shortLabels: Record<string, string> = {
    chat_completions: "Chat",
    responses: "Responses",
    anthropic_messages: "Anthropic",
    google_gemini: "Gemini",
};

const shortLabel = (mode: string) => shortLabels[mode] ?? requestModeLabels[mode] ?? mode;

export default function ModelRequestModeToggle({
    requestMode, availableModes, onToggle,
}: Props) {
    const choices = availableModes ?? ["chat_completions", "responses"];
    if (choices.length < 2) {
        return <span className="shrink-0 rounded px-1.5 py-0.5 text-xs text-muted-foreground"
            title={requestModeLabels[choices[0] ?? "unsupported"]}>
            {choices[0] ? shortLabel(choices[0]) : requestModeLabels.unsupported}
        </span>;
    }
    const recommended = availableModes ? choices[0] : undefined;
    return <DropdownMenu modal={false}>
        <DropdownMenuTrigger
            aria-label={`请求协议：${requestModeLabels[requestMode] ?? requestMode}`}
            title={requestModeLabels[requestMode] ?? requestMode}
            onClick={event => event.stopPropagation()}
            className="inline-flex h-6 shrink-0 cursor-pointer items-center gap-0.5 rounded px-1.5 text-xs text-muted-foreground transition-colors outline-none hover:bg-background hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring data-[state=open]:bg-background data-[state=open]:text-foreground">
            {shortLabel(requestMode)}
            <ChevronDown className="h-3 w-3 opacity-60" />
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-[13rem]">
            <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">请求协议</DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuRadioGroup value={requestMode} onValueChange={onToggle}>
                {choices.map(mode => <DropdownMenuRadioItem key={mode} value={mode} className="text-xs">
                    <span className="flex-1">{requestModeLabels[mode] ?? mode}</span>
                    {mode === recommended && <span className="rounded border border-border px-1 text-[10px] leading-4 text-muted-foreground">
                        推荐
                    </span>}
                </DropdownMenuRadioItem>)}
            </DropdownMenuRadioGroup>
        </DropdownMenuContent>
    </DropdownMenu>;
}
