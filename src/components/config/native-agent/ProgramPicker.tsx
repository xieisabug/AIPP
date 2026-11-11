import React, { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, CheckCircle2, FileSearch, Loader2, XCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { DetectedProgram } from "@/hooks/feature/useNativeAgentEnvironment";

const AUTO = "__auto__";
const CUSTOM = "__custom__";

/** Paths from detection vs. the stored value: Windows paths compare case/slash-insensitively. */
export function samePath(a: string, b: string): boolean {
    const normalize = (value: string) => {
        const text = value.trim().replace(/[\\/]+$/, "");
        return /^[a-zA-Z]:[\\/]|\\/.test(text) ? text.replace(/\//g, "\\").toLowerCase() : text;
    };
    return normalize(a) === normalize(b);
}

function describe(program: DetectedProgram): string {
    if (program.version) return `v${program.version} · ${program.source}`;
    return program.source;
}

/** Version/path line for what will actually run, with warnings and errors spelled out. */
export const ProgramStatus: React.FC<{ program: DetectedProgram | null; detecting?: boolean }> = ({ program, detecting }) => {
    if (!program) {
        return detecting ? (
            <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />正在检测…
            </p>
        ) : null;
    }
    return (
        <div className="flex flex-col gap-1 text-xs">
            {program.ok ? (
                <p className="flex min-w-0 items-start gap-1.5 text-muted-foreground">
                    <CheckCircle2 className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                    <span className="min-w-0 break-all">
                        <span className="font-medium text-foreground">{program.version ? `v${program.version}` : "版本未知"}</span>
                        {" · "}{program.path}
                    </span>
                </p>
            ) : (
                <p className="flex min-w-0 items-start gap-1.5 text-destructive">
                    <XCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                    <span className="min-w-0 whitespace-pre-wrap break-all">{program.error ?? `无法运行 ${program.path}`}</span>
                </p>
            )}
            {program.warning && (
                <p className="flex items-start gap-1.5 text-foreground">
                    <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                    <span>{program.warning}</span>
                </p>
            )}
        </div>
    );
};

/** Absolute file path input with a native file browser. */
export const PathInput: React.FC<{
    value: string;
    onChange: (value: string) => void;
    placeholder?: string;
    browseTitle: string;
    disabled?: boolean;
}> = ({ value, onChange, placeholder, browseTitle, disabled }) => (
    <div className="flex items-center gap-2">
        <Input
            className="flex-1 font-mono text-xs"
            value={value}
            placeholder={placeholder}
            disabled={disabled}
            onChange={(event) => onChange(event.target.value)}
        />
        <Button
            type="button"
            variant="outline"
            size="icon"
            className="shrink-0"
            aria-label={browseTitle}
            title={browseTitle}
            disabled={disabled}
            onClick={async () => {
                const selected = await open({ directory: false, multiple: false, title: browseTitle });
                if (typeof selected === "string") onChange(selected);
            }}
        >
            <FileSearch className="h-4 w-4" />
        </Button>
    </div>
);

interface ProgramPickerProps {
    /** Stored configuration; empty (or one of autoValues) means "find it on PATH". */
    value: string;
    autoValues?: string[];
    autoLabel: string;
    /** What the current value resolves to. */
    selected: DetectedProgram | null;
    candidates: DetectedProgram[];
    detecting?: boolean;
    onChange: (value: string) => void;
    browseTitle: string;
    customPlaceholder: string;
    ariaLabel: string;
}

/**
 * Choose between automatic PATH lookup, a detected installation, or a custom path.
 * The stored value stays a plain command/path string, so existing configs keep working.
 */
export const ProgramPicker: React.FC<ProgramPickerProps> = ({
    value,
    autoValues = [""],
    autoLabel,
    selected,
    candidates,
    detecting,
    onChange,
    browseTitle,
    customPlaceholder,
    ariaLabel,
}) => {
    const trimmed = value.trim();
    const isAuto = autoValues.includes(trimmed);
    const matchIndex = isAuto ? -1 : candidates.findIndex((candidate) => samePath(candidate.path, trimmed));
    const [customMode, setCustomMode] = useState(!isAuto && matchIndex < 0);
    useEffect(() => {
        if (isAuto || matchIndex >= 0) setCustomMode(false);
        else if (trimmed) setCustomMode(true);
    }, [isAuto, matchIndex, trimmed]);

    const selectValue = customMode ? CUSTOM : isAuto ? AUTO : matchIndex >= 0 ? `candidate:${matchIndex}` : CUSTOM;
    const triggerLabel = useMemo(() => {
        if (selectValue === AUTO) return selected?.ok && selected.version ? `自动 · v${selected.version}` : "自动";
        if (selectValue === CUSTOM) return "自定义路径";
        return describe(candidates[matchIndex]);
    }, [selectValue, selected, candidates, matchIndex]);

    return (
        <div className="flex flex-col gap-2">
            <Select
                value={selectValue}
                onValueChange={(next) => {
                    if (next === AUTO) { setCustomMode(false); onChange(""); }
                    else if (next === CUSTOM) setCustomMode(true);
                    else { setCustomMode(false); onChange(candidates[Number(next.slice("candidate:".length))].path); }
                }}
            >
                <SelectTrigger aria-label={ariaLabel} className="w-full">
                    <SelectValue>{triggerLabel}</SelectValue>
                </SelectTrigger>
                <SelectContent>
                    <SelectItem value={AUTO}>
                        <span className="flex min-w-0 flex-col items-start">
                            <span>{autoLabel}</span>
                            <span className="max-w-[28rem] truncate text-xs text-muted-foreground">
                                {isAuto && selected ? (selected.ok ? `${selected.version ? `v${selected.version} · ` : ""}${selected.path}` : selected.error) : "按 PATH 查找"}
                            </span>
                        </span>
                    </SelectItem>
                    {candidates.map((candidate, index) => (
                        <SelectItem key={candidate.path} value={`candidate:${index}`}>
                            <span className="flex min-w-0 flex-col items-start">
                                <span>{describe(candidate)}{candidate.warning ? `（${candidate.warning}）` : ""}{!candidate.ok && candidate.error ? "（不可用）" : ""}</span>
                                <span className="max-w-[28rem] truncate text-xs text-muted-foreground">{candidate.path}</span>
                            </span>
                        </SelectItem>
                    ))}
                    <SelectItem value={CUSTOM}>自定义路径…</SelectItem>
                </SelectContent>
            </Select>
            {selectValue === CUSTOM && (
                <PathInput
                    value={isAuto ? "" : value}
                    onChange={onChange}
                    placeholder={customPlaceholder}
                    browseTitle={browseTitle}
                />
            )}
            <ProgramStatus program={selected} detecting={detecting} />
        </div>
    );
};
