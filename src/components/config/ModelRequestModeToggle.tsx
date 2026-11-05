import { requestModeLabels } from "./llmModelTypes";

interface Props {
    requestMode: string;
    availableModes?: string[];
    onToggle: (mode: string) => void;
}

export default function ModelRequestModeToggle({
    requestMode, availableModes, onToggle,
}: Props) {
    const choices = availableModes ?? ["chat_completions", "responses"];
    if (choices.length < 2) {
        return <span className="ml-1 text-xs text-muted-foreground">
            {requestModeLabels[choices[0] ?? "unsupported"]}
        </span>;
    }
    return <select aria-label="请求协议" value={requestMode}
        className="ml-1 max-w-full rounded border bg-background px-1 text-xs text-foreground"
        onClick={event => event.stopPropagation()}
        onChange={event => onToggle(event.target.value)}>
        {choices.map((mode, index) => <option key={mode} value={mode}>
            {requestModeLabels[mode]}{availableModes && index === 0 ? "（推荐）" : ""}
        </option>)}
    </select>;
}
