import React, { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
    Sparkles,
    Loader2,
    RefreshCw,
    ExternalLink,
    ShieldAlert,
    CheckCircle2,
    XCircle,
    Download,
    BookOpen,
} from "lucide-react";
import SkillInstallGuideDialog from "@/components/config/SkillInstallGuideDialog";

// --- Types (mirrors SkillInstallGuideDialog) ---

interface SkillInstallRecipeSource {
    type: "github" | "zip";
    repo?: string | null;
    ref: string;
    url?: string | null;
}

interface SkillInstallRecipeDir {
    from: string;
    to: string;
}

interface SkillInstallPlanSkill {
    from: string;
    to: string;
    display_name: string;
    detected_entry_file: string;
    normalized_entry_file: string;
    will_replace: boolean;
    metadata: {
        name?: string | null;
        description?: string | null;
        version?: string | null;
        author?: string | null;
        tags: string[];
        requires_files: string[];
    };
    preview?: string | null;
}

interface OfficialSkill {
    id: string;
    name: string;
    description: string;
    version?: string | null;
    source?: SkillInstallRecipeSource | null;
    dirs?: SkillInstallRecipeDir[] | null;
    download_url?: string | null;
    source_url?: string | null;
}

type FetchStatus = "idle" | "loading" | "success" | "error" | "timeout";
type SkillInstallStatus = "idle" | "inspecting" | "installing" | "installed" | "failed";

interface StepSkillsInstallProps {
    onSkillInstalled?: () => void;
}

function resolveSource(skill: OfficialSkill): SkillInstallRecipeSource | null {
    if (skill.source) return skill.source;
    if (skill.download_url) return { type: "zip", ref: "main", url: skill.download_url };
    return null;
}

const StepSkillsInstall: React.FC<StepSkillsInstallProps> = ({
    onSkillInstalled,
}) => {
    const [officialSkills, setOfficialSkills] = useState<OfficialSkill[]>([]);
    const [fetchStatus, setFetchStatus] = useState<FetchStatus>("idle");
    const [fetchError, setFetchError] = useState("");
    const [installGuideOpen, setInstallGuideOpen] = useState(false);

    // Per-skill install status
    const [installStatusMap, setInstallStatusMap] = useState<
        Record<string, SkillInstallStatus>
    >({});
    const [installErrorMap, setInstallErrorMap] = useState<Record<string, string>>({});

    const fetchOfficialSkills = useCallback(async (useProxy: boolean) => {
        setFetchStatus("loading");
        setFetchError("");
        try {
            const skills = await invoke<OfficialSkill[]>("fetch_official_skills", {
                useProxy,
            });
            setOfficialSkills(skills);
            setFetchStatus("success");
        } catch (error) {
            const message = String(error);
            setFetchError(message);
            setFetchStatus(
                message.includes("超时") || message.toLowerCase().includes("timeout")
                    ? "timeout"
                    : "error"
            );
        }
    }, []);

    useEffect(() => {
        if (fetchStatus === "idle") {
            void fetchOfficialSkills(false);
        }
    }, [fetchOfficialSkills, fetchStatus]);

    const handleInstallSkill = useCallback(
        async (skill: OfficialSkill, useProxy = false) => {
            const source = resolveSource(skill);
            if (!source) {
                toast.error(`${skill.name} 缺少可用的下载源`);
                return;
            }

            setInstallStatusMap((prev) => ({ ...prev, [skill.id]: "inspecting" }));
            setInstallErrorMap((prev) => {
                const next = { ...prev };
                delete next[skill.id];
                return next;
            });

            try {
                // 1. Inspect
                const inspection = await invoke<{
                    source: SkillInstallRecipeSource;
                    source_label: string;
                    download_url: string;
                    target_directory: string;
                    skills: SkillInstallPlanSkill[];
                }>("inspect_skill_archive_source", {
                    source,
                    dirs: skill.dirs?.length ? skill.dirs : null,
                    useProxy,
                });

                if (inspection.skills.length === 0) {
                    setInstallStatusMap((prev) => ({ ...prev, [skill.id]: "failed" }));
                    setInstallErrorMap((prev) => ({
                        ...prev,
                        [skill.id]: "未发现可安装的 Skill",
                    }));
                    return;
                }

                // 2. Auto-select all → install
                setInstallStatusMap((prev) => ({ ...prev, [skill.id]: "installing" }));

                const selections = inspection.skills.map((s) => ({
                    from: s.from,
                    to: s.to,
                }));

                const result = await invoke<{
                    installed_skills: SkillInstallPlanSkill[];
                }>("install_skill_archive_source", {
                    source,
                    selections,
                    useProxy,
                });

                setInstallStatusMap((prev) => ({
                    ...prev,
                    [skill.id]: "installed",
                }));
                toast.success(`已安装 ${result.installed_skills.length} 个 Skill`);
                onSkillInstalled?.();
            } catch (error) {
                const message = String(error);
                setInstallStatusMap((prev) => ({ ...prev, [skill.id]: "failed" }));
                setInstallErrorMap((prev) => ({ ...prev, [skill.id]: message }));
            }
        },
        [onSkillInstalled]
    );

    const handleOpenSourceUrl = useCallback(async (url?: string | null) => {
        if (!url) return;
        try {
            await invoke("open_source_url", { url });
        } catch (error) {
            console.error("打开来源链接失败:", error);
        }
    }, []);

    return (
        <div className="space-y-6">
            <div className="space-y-2">
                <div className="flex items-center gap-2 text-primary">
                    <Sparkles className="h-5 w-5" />
                    <h3 className="text-lg font-semibold">安装推荐 Skills</h3>
                </div>
                <p className="text-sm text-muted-foreground leading-relaxed">
                    Skills 是 AI 可调用的技能和指令，能显著增强总管家的任务处理能力。
                    以下是我们推荐的 Skills，你可以选择安装，也可以跳过此步骤稍后再安装。
                </p>
                <p className="text-xs text-muted-foreground">
                    推荐安装 AIPP 官方 Skills 和 Vercel Labs find-skills，以获得更丰富的
                    AI 能力。
                </p>
            </div>

            {/* Loading state */}
            {fetchStatus === "loading" && (
                <div className="flex flex-col items-center justify-center py-8 text-muted-foreground gap-2">
                    <Loader2 className="h-6 w-6 animate-spin" />
                    <span className="text-sm">加载推荐 Skills...</span>
                </div>
            )}

            {/* Error state */}
            {(fetchStatus === "error" || fetchStatus === "timeout") && (
                <div className="rounded-lg border border-border/60 bg-muted/30 p-4 space-y-3">
                    <div className="flex items-start gap-3">
                        <ShieldAlert className="h-5 w-5 text-muted-foreground mt-0.5" />
                        <div className="space-y-1 min-w-0">
                            <div className="text-sm font-medium">
                                {fetchStatus === "timeout" ? "请求超时" : "加载推荐列表失败"}
                            </div>
                            <div className="text-xs text-muted-foreground break-words">
                                {fetchError}
                            </div>
                        </div>
                    </div>
                    <div className="flex flex-wrap gap-2">
                        <Button
                            variant="outline"
                            size="sm"
                            onClick={() => void fetchOfficialSkills(false)}
                        >
                            重试
                        </Button>
                        <Button
                            size="sm"
                            onClick={() => void fetchOfficialSkills(true)}
                            className="gap-1"
                        >
                            <RefreshCw className="h-3.5 w-3.5" />
                            使用代理
                        </Button>
                    </div>
                </div>
            )}

            {/* Skills list */}
            {fetchStatus === "success" && officialSkills.length > 0 && (
                <div className="space-y-3">
                    {officialSkills.map((skill) => {
                        const status = installStatusMap[skill.id] ?? "idle";
                        const error = installErrorMap[skill.id];
                        const hasSource = !!resolveSource(skill);

                        return (
                            <div
                                key={skill.id}
                                className="rounded-lg border border-border/60 bg-muted/30 p-4 space-y-2"
                            >
                                <div className="flex items-start justify-between gap-3">
                                    <div className="flex-1 space-y-1">
                                        <div className="flex items-center gap-2">
                                            <span className="font-medium">{skill.name}</span>
                                            {skill.version && (
                                                <Badge variant="outline" className="text-xs">
                                                    v{skill.version}
                                                </Badge>
                                            )}
                                            {status === "installed" && (
                                                <span className="inline-flex items-center gap-1 text-xs text-green-600 dark:text-green-400">
                                                    <CheckCircle2 className="h-3.5 w-3.5" />
                                                    已安装
                                                </span>
                                            )}
                                        </div>
                                        <p className="text-xs text-muted-foreground">
                                            {skill.description}
                                        </p>
                                    </div>
                                    <div className="flex items-center gap-2 shrink-0">
                                        {skill.source_url && (
                                            <Button
                                                variant="ghost"
                                                size="sm"
                                                onClick={() =>
                                                    void handleOpenSourceUrl(skill.source_url)
                                                }
                                            >
                                                <ExternalLink className="h-3.5 w-3.5" />
                                            </Button>
                                        )}
                                        {/* Install button */}
                                        {status === "inspecting" ? (
                                            <Button variant="outline" size="sm" disabled>
                                                <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
                                                识别中
                                            </Button>
                                        ) : status === "installing" ? (
                                            <Button variant="outline" size="sm" disabled>
                                                <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
                                                安装中
                                            </Button>
                                        ) : status === "installed" ? (
                                            <Button variant="ghost" size="sm" disabled>
                                                <CheckCircle2 className="h-3.5 w-3.5 mr-1.5" />
                                                已完成
                                            </Button>
                                        ) : (
                                            <Button
                                                variant="default"
                                                size="sm"
                                                disabled={!hasSource}
                                                onClick={() =>
                                                    void handleInstallSkill(skill)
                                                }
                                            >
                                                <Download className="h-3.5 w-3.5 mr-1.5" />
                                                一键安装
                                            </Button>
                                        )}
                                    </div>
                                </div>

                                {/* Error with retry */}
                                {status === "failed" && error && (
                                    <div className="flex items-start gap-2 text-xs text-destructive">
                                        <XCircle className="h-3.5 w-3.5 mt-0.5 shrink-0" />
                                        <div className="flex-1 space-y-1">
                                            <span>{error}</span>
                                            <div className="flex gap-2 pt-1">
                                                <Button
                                                    variant="outline"
                                                    size="sm"
                                                    className="h-6 text-xs"
                                                    onClick={() =>
                                                        void handleInstallSkill(skill)
                                                    }
                                                >
                                                    重试
                                                </Button>
                                                <Button
                                                    variant="outline"
                                                    size="sm"
                                                    className="h-6 text-xs gap-1"
                                                    onClick={() =>
                                                        void handleInstallSkill(skill, true)
                                                    }
                                                >
                                                    <RefreshCw className="h-3 w-3" />
                                                    使用代理重试
                                                </Button>
                                            </div>
                                        </div>
                                    </div>
                                )}
                            </div>
                        );
                    })}
                </div>
            )}

            {fetchStatus === "success" && officialSkills.length === 0 && (
                <div className="flex flex-col items-center justify-center py-6 text-muted-foreground gap-2">
                    <p className="text-sm">暂无推荐 Skills</p>
                </div>
            )}

            {/* More skills link */}
            <div className="flex justify-center pt-2">
                <Button
                    variant="outline"
                    onClick={() => setInstallGuideOpen(true)}
                    className="gap-1.5"
                >
                    <BookOpen className="h-4 w-4" />
                    更多 Skills 安装选项
                </Button>
            </div>

            <SkillInstallGuideDialog
                isOpen={installGuideOpen}
                onClose={() => setInstallGuideOpen(false)}
                onSkillInstalled={() => {
                    onSkillInstalled?.();
                    void fetchOfficialSkills(false);
                }}
            />
        </div>
    );
};

export default StepSkillsInstall;
