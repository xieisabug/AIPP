import React, { useEffect, useState } from "react";
import { Code2, Plus, Table2, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";

export interface EnvVarRow {
    key: string;
    value: string;
}

export type EnvVarFormat = "dotenv" | "json";

/** Parse either a JSON object string or KEY=VALUE lines into key-value rows. */
export function parseEnvVars(raw: string): EnvVarRow[] {
    const trimmed = (raw || "").trim();
    if (!trimmed) return [];

    if (trimmed.startsWith("{")) {
        try {
            const parsed = JSON.parse(trimmed);
            if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
                return Object.entries(parsed).map(([key, value]) => ({
                    key,
                    value: value == null ? "" : String(value),
                }));
            }
        } catch {
            // Fall through to dotenv parsing
        }
    }

    const rows: EnvVarRow[] = [];
    for (const rawLine of trimmed.split(/\r?\n/)) {
        const line = rawLine.trim();
        if (!line || line.startsWith("#")) continue;
        const eq = line.indexOf("=");
        if (eq <= 0) {
            rows.push({ key: line, value: "" });
            continue;
        }
        const key = line.slice(0, eq).trim();
        let val = line.slice(eq + 1).trim();
        if (
            (val.startsWith("\"") && val.endsWith("\"")) ||
            (val.startsWith("'") && val.endsWith("'"))
        ) {
            val = val.slice(1, -1);
        }
        rows.push({ key, value: val });
    }
    return rows;
}

export function serializeEnvVars(rows: EnvVarRow[], format: EnvVarFormat): string {
    const valid = rows.filter((r) => r.key.trim().length > 0);
    if (valid.length === 0) return "";
    if (format === "json") {
        const obj: Record<string, string> = {};
        for (const r of valid) {
            obj[r.key.trim()] = r.value;
        }
        return JSON.stringify(obj, null, 2);
    }
    return valid.map((r) => `${r.key.trim()}=${r.value}`).join("\n");
}

interface EnvVarEditorProps {
    value: string;
    onChange: (next: string) => void;
    format: EnvVarFormat;
    disabled?: boolean;
    placeholder?: string;
    ariaLabel?: string;
}

export const EnvVarEditor: React.FC<EnvVarEditorProps> = ({
    value,
    onChange,
    format,
    disabled = false,
    placeholder,
    ariaLabel = "环境变量",
}) => {
    const [rawMode, setRawMode] = useState(false);
    const [rows, setRows] = useState<EnvVarRow[]>(() => parseEnvVars(value));

    useEffect(() => {
        if (rawMode) return;
        const currentSerialized = serializeEnvVars(rows, format);
        if (currentSerialized !== (value || "").trim()) {
            setRows(parseEnvVars(value));
        }
    }, [value, format, rawMode]);

    const commitRows = (nextRows: EnvVarRow[]) => {
        setRows(nextRows);
        onChange(serializeEnvVars(nextRows, format));
    };

    const handlePasteIntoKey = (index: number, event: React.ClipboardEvent<HTMLInputElement>) => {
        const pasted = event.clipboardData.getData("text");
        if (!pasted) return;
        const isMulti = pasted.includes("\n") || pasted.trim().startsWith("{") || pasted.includes("=");
        if (!isMulti) return;
        const parsed = parseEnvVars(pasted);
        if (parsed.length === 0) return;
        event.preventDefault();
        const next = [...rows];
        next.splice(index, 1, ...parsed);
        commitRows(next);
    };

    if (rawMode) {
        return (
            <div className="space-y-2">
                <div className="flex items-center justify-between">
                    <span className="text-xs text-muted-foreground">
                        {format === "json" ? "JSON 对象格式" : "每行一个 KEY=VALUE（也支持粘贴 JSON）"}
                    </span>
                    <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="h-7 gap-1 px-2 text-xs"
                        disabled={disabled}
                        onClick={() => {
                            const parsed = parseEnvVars(value);
                            setRows(parsed);
                            setRawMode(false);
                        }}
                    >
                        <Table2 className="h-3.5 w-3.5" />
                        可视化编辑
                    </Button>
                </div>
                <Textarea
                    value={value || ""}
                    disabled={disabled}
                    onChange={(e) => onChange(e.target.value)}
                    placeholder={
                        placeholder ??
                        (format === "json"
                            ? '{"ANTHROPIC_BASE_URL":"https://..."}'
                            : "KEY=VALUE\nANOTHER_KEY=VALUE")
                    }
                    className="min-h-28 font-mono text-xs"
                    aria-label={ariaLabel}
                    spellCheck={false}
                />
            </div>
        );
    }

    return (
        <div className="space-y-2" data-aipp-slot="env-var-editor">
            <div className="flex items-center justify-between gap-2">
                <span className="text-xs text-muted-foreground">
                    可直接粘贴多行 <code className="font-mono">.env</code> 或 JSON 自动拆分
                </span>
                <div className="flex items-center gap-1">
                    <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="h-7 gap-1 px-2 text-xs"
                        disabled={disabled}
                        onClick={() => setRawMode(true)}
                    >
                        <Code2 className="h-3.5 w-3.5" />
                        纯文本
                    </Button>
                    <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        className="h-7 gap-1 px-2 text-xs"
                        disabled={disabled}
                        onClick={() => setRows([...rows, { key: "", value: "" }])}
                    >
                        <Plus className="h-3.5 w-3.5" />
                        添加变量
                    </Button>
                </div>
            </div>

            {rows.length === 0 ? (
                <div className="rounded-md border border-dashed border-border px-3 py-3 text-center text-xs text-muted-foreground">
                    暂未配置覆盖环境变量
                </div>
            ) : (
                <div className="space-y-1.5">
                    {rows.map((row, index) => (
                        <div key={index} className="flex items-center gap-2">
                            <Input
                                value={row.key}
                                disabled={disabled}
                                placeholder="变量名（如 API_BASE_URL）"
                                className="h-8 flex-1 font-mono text-xs"
                                onPaste={(e) => handlePasteIntoKey(index, e)}
                                onChange={(e) => {
                                    const next = rows.map((r, i) => (i === index ? { ...r, key: e.target.value } : r));
                                    commitRows(next);
                                }}
                            />
                            <Input
                                value={row.value}
                                disabled={disabled}
                                placeholder="值"
                                className="h-8 flex-1 font-mono text-xs"
                                onChange={(e) => {
                                    const next = rows.map((r, i) => (i === index ? { ...r, value: e.target.value } : r));
                                    commitRows(next);
                                }}
                            />
                            <Button
                                type="button"
                                variant="ghost"
                                size="icon"
                                disabled={disabled}
                                aria-label="删除环境变量"
                                onClick={() => commitRows(rows.filter((_, i) => i !== index))}
                                className="h-8 w-8 shrink-0 text-muted-foreground hover:text-destructive"
                            >
                                <Trash2 className="h-3.5 w-3.5" />
                            </Button>
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
};

export default EnvVarEditor;
