import React, { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { CheckCircle2, ChevronLeft, ChevronRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    CompactDialogFooter,
} from "@/components/ui/dialog";
import { useFilteredModels } from "@/hooks/useFilteredModels";
import { saveButlerModel } from "./ButlerModelOptions";
import type { SaveFeatureConfigFn } from "@/components/config/feature/forms/experimentalConfigShared";
import { useButlerOnboarding } from "./useButlerOnboarding";
import {
    serializeButlerWorkspaceConfig,
    type TrustedWorkspace,
} from "./butlerWorkspaceConfig";
import StepModelConfig from "./steps/StepModelConfig";
import StepEnvironmentCheck from "./steps/StepEnvironmentCheck";
import StepSkillsInstall from "./steps/StepSkillsInstall";
import StepWorkspaceConfig from "./steps/StepWorkspaceConfig";
import StepFeishuGuide from "./steps/StepFeishuGuide";

const STEP_LABELS = ["模型配置", "环境检测", "安装 Skills", "工作区", "飞书接入"];

interface ButlerOnboardingWizardProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    existingModelId?: string;
    existingDisplayName?: string;
    existingTrustAll?: boolean;
    existingMainWorkspace?: TrustedWorkspace | null;
    existingTrustedWorkspaces?: TrustedWorkspace[];
    existingFeishuEnabled?: boolean;
    existingFeishuAppId?: string;
    existingFeishuBaseUrl?: string;
    saveFeatureConfig: SaveFeatureConfigFn;
    onComplete: () => void;
}

export const ButlerOnboardingWizard: React.FC<ButlerOnboardingWizardProps> = ({
    open,
    onOpenChange,
    existingModelId,
    existingDisplayName,
    existingTrustAll,
    existingMainWorkspace,
    existingTrustedWorkspaces,
    existingFeishuEnabled,
    existingFeishuAppId,
    existingFeishuBaseUrl,
    saveFeatureConfig,
    onComplete,
}) => {
    const [saving, setSaving] = useState(false);

    const {
        state,
        setModelId,
        setDisplayName,
        checkBunVersion,
        checkUvVersion,
        installBun,
        installUv,
        setMainWorkspacePath,
        setMainWorkspaceDescription,
        setTrustAllWorkspaces,
        addTrustedWorkspace,
        removeTrustedWorkspace,
        updateWorkspaceDescription,
        setFeishuEnabled,
        setFeishuAppId,
        setFeishuAppSecret,
        setFeishuBaseUrl,
        goNext,
        goPrev,
        canGoNext,
        canGoPrev,
        isLastStep,
        isStepValid,
    } = useButlerOnboarding({
        isOpen: open,
        existingModelId,
        existingDisplayName,
        existingTrustAll,
        existingMainWorkspace,
        existingTrustedWorkspaces,
        existingFeishuEnabled,
        existingFeishuAppId,
        existingFeishuBaseUrl,
    });

    const api = useFilteredModels(0, open);
    const agents = useFilteredModels(4, open);

    const persistWizard = useCallback(async (closeAfterSave: boolean) => {
        if (!closeAfterSave && !state.mainWorkspacePath.trim()) return true;
        setSaving(true);
        try {
            const workspaceConfig = serializeButlerWorkspaceConfig({
                mainWorkspacePath: state.mainWorkspacePath,
                mainWorkspaceDescription: state.mainWorkspaceDescription,
                trustedWorkspaces: state.trustedWorkspaces,
            });
            if (!await saveButlerModel(state.modelId, () => saveFeatureConfig("experimental", {
                butler_experiment_enabled: "true",
                butler_model_id: state.modelId,
                butler_display_name: state.displayName || "总管家",
                butler_main_workspace_path: workspaceConfig.mainWorkspacePath,
                butler_main_workspace_description: workspaceConfig.mainWorkspaceDescription,
                butler_trust_all_workspaces: String(state.trustAllWorkspaces),
                butler_trusted_workspaces: workspaceConfig.trustedWorkspacesRaw,
                butler_feishu_enabled: String(state.feishuEnabled),
                butler_feishu_app_id: state.feishuAppId,
                butler_feishu_base_url: state.feishuBaseUrl,
            }, true))) {
                setModelId(existingModelId ?? "");
                return false;
            }

            // Save Feishu secret separately if provided
            if (state.feishuEnabled && state.feishuAppSecret.trim()) {
                await invoke("save_butler_feishu_secret", {
                    appSecret: state.feishuAppSecret.trim(),
                    app_secret: state.feishuAppSecret.trim(),
                });
            }
            await invoke("refresh_butler_feishu_runtime_command");

            if (closeAfterSave) {
                toast.success("总管家配置已保存");
                onOpenChange(false);
                onComplete();
            } else {
                toast.success("当前步骤已保存");
            }
            return true;
        } catch (err) {
            toast.error(`保存失败: ${err}`);
            return false;
        } finally {
            setSaving(false);
        }
    }, [onComplete, onOpenChange, saveFeatureConfig, state]);

    const handleNext = useCallback(async () => {
        if (isLastStep) {
            await persistWizard(true);
        } else {
            const saved = await persistWizard(false);
            if (!saved) {
                return;
            }
            goNext();
        }
    }, [goNext, isLastStep, persistWizard]);

    const handleSkip = useCallback(async () => {
        const saved = await persistWizard(false);
        if (!saved) {
            return;
        }
        goNext();
    }, [goNext, persistWizard]);

    const currentStepValid = isStepValid(state.currentStep);

    return (
        <Dialog open={open} onOpenChange={(next) => { if (!saving) onOpenChange(next); }}>
            <CompactDialogContent className="sm:max-w-2xl">
                <CompactDialogHeader
                    title="总管家设置向导"
                    subtitle={`${state.currentStep + 1}/${STEP_LABELS.length} · ${STEP_LABELS[state.currentStep]}`}
                    description="按步骤完成总管家的基本配置"
                    closeDisabled={saving}
                >
                    <div
                        aria-hidden
                        className="absolute bottom-[-1px] left-0 h-0.5 bg-foreground/70 transition-[width] duration-300"
                        style={{ width: `${((state.currentStep + 1) / STEP_LABELS.length) * 100}%` }}
                    />
                </CompactDialogHeader>

                {/* Step indicator */}
                <div className="flex shrink-0 items-center justify-center gap-1 border-b border-border/60 bg-muted/20 px-4 py-2 sm:px-5">
                    {STEP_LABELS.map((label, index) => (
                        <React.Fragment key={index}>
                            {index > 0 && (
                                <div
                                    className={`h-px w-6 transition-colors ${
                                        index <= state.currentStep
                                            ? "bg-foreground/50"
                                            : "bg-border"
                                    }`}
                                />
                            )}
                            <span
                                className={`flex items-center gap-1.5 rounded-md px-2 py-0.5 text-xs font-medium transition-colors ${
                                    index === state.currentStep
                                        ? "bg-muted text-foreground"
                                        : index < state.currentStep
                                          ? "text-muted-foreground"
                                          : "text-muted-foreground/70"
                                }`}
                            >
                                {index < state.currentStep ? (
                                    <CheckCircle2 className="h-3.5 w-3.5 text-foreground/70" />
                                ) : (
                                    <span
                                        className={`flex h-4 w-4 items-center justify-center rounded-full text-[10px] ${
                                            index === state.currentStep
                                                ? "bg-foreground text-background"
                                                : "bg-muted text-muted-foreground"
                                        }`}
                                    >
                                        {index + 1}
                                    </span>
                                )}
                                <span className="hidden sm:inline">{label}</span>
                            </span>
                        </React.Fragment>
                    ))}
                </div>

                {/* Step content */}
                <CompactDialogBody>
                    {state.currentStep === 0 && (
                        <StepModelConfig
                            modelId={state.modelId}
                            displayName={state.displayName}
                            apiModels={api.models}
                            agentModels={agents.models}
                            modelsLoading={api.loading || agents.loading}
                            onModelChange={setModelId}
                            onDisplayNameChange={setDisplayName}
                        />
                    )}
                    {state.currentStep === 1 && (
                        <StepEnvironmentCheck
                            bunVersion={state.bunVersion}
                            uvVersion={state.uvVersion}
                            bunInstalling={state.bunInstalling}
                            uvInstalling={state.uvInstalling}
                            bunInstallLog={state.bunInstallLog}
                            uvInstallLog={state.uvInstallLog}
                            onCheckBun={checkBunVersion}
                            onCheckUv={checkUvVersion}
                            onInstallBun={installBun}
                            onInstallUv={installUv}
                        />
                    )}
                    {state.currentStep === 2 && (
                        <StepSkillsInstall />
                    )}
                    {state.currentStep === 3 && (
                        <StepWorkspaceConfig
                            mainWorkspacePath={state.mainWorkspacePath}
                            mainWorkspaceDescription={state.mainWorkspaceDescription}
                            trustedWorkspaces={state.trustedWorkspaces}
                            trustAllWorkspaces={state.trustAllWorkspaces}
                            onMainWorkspacePathChange={setMainWorkspacePath}
                            onMainWorkspaceDescriptionChange={setMainWorkspaceDescription}
                            onTrustAllChange={setTrustAllWorkspaces}
                            onAddWorkspace={addTrustedWorkspace}
                            onRemoveWorkspace={removeTrustedWorkspace}
                            onUpdateDescription={updateWorkspaceDescription}
                        />
                    )}
                    {state.currentStep === 4 && (
                        <StepFeishuGuide
                            feishuEnabled={state.feishuEnabled}
                            feishuAppId={state.feishuAppId}
                            feishuAppSecret={state.feishuAppSecret}
                            feishuBaseUrl={state.feishuBaseUrl}
                            onEnabledChange={setFeishuEnabled}
                            onAppIdChange={setFeishuAppId}
                            onAppSecretChange={setFeishuAppSecret}
                            onBaseUrlChange={setFeishuBaseUrl}
                        />
                    )}
                </CompactDialogBody>

                {/* Footer navigation */}
                <CompactDialogFooter className="justify-between">
                    <div>
                        {canGoPrev && (
                            <Button
                                type="button"
                                variant="ghost"
                                className="min-h-11 gap-1 px-3 sm:min-h-9"
                                onClick={goPrev}
                                disabled={saving}
                            >
                                <ChevronLeft className="h-4 w-4" />
                                上一步
                            </Button>
                        )}
                    </div>
                    <div className="flex items-center gap-2">
                        {canGoNext && state.currentStep > 0 && (
                            <Button
                                type="button"
                                variant="ghost"
                                className="min-h-11 px-3 sm:min-h-9"
                                onClick={() => void handleSkip()}
                                disabled={saving || state.currentStep === 3}
                            >
                                跳过
                            </Button>
                        )}
                        <Button
                            type="button"
                            className="min-h-11 gap-1 px-5 sm:min-h-9"
                            onClick={() => void handleNext()}
                            disabled={!currentStepValid || saving}
                        >
                            {saving ? (
                                "保存中..."
                            ) : isLastStep ? (
                                "完成设置"
                            ) : (
                                <>
                                    下一步
                                    <ChevronRight className="h-4 w-4" />
                                </>
                            )}
                        </Button>
                    </div>
                </CompactDialogFooter>
            </CompactDialogContent>
        </Dialog>
    );
};

export default ButlerOnboardingWizard;
