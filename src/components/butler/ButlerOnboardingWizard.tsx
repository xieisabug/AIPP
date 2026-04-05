import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { CheckCircle2, ChevronLeft, ChevronRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import { useModels } from "@/hooks/useModels";
import { saveExperimentalConfigValues } from "@/components/config/feature/forms/experimentalConfigShared";
import {
    useButlerOnboarding,
    type TrustedWorkspace,
} from "./useButlerOnboarding";
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
    existingTrustedWorkspaces?: TrustedWorkspace[];
    existingFeishuEnabled?: boolean;
    existingFeishuAppId?: string;
    existingFeishuBaseUrl?: string;
    initialValues?: Record<string, unknown>;
    saveFeatureConfig: (featureCode: string, config: Record<string, unknown>) => Promise<unknown>;
    onComplete: () => void;
}

export const ButlerOnboardingWizard: React.FC<ButlerOnboardingWizardProps> = ({
    open,
    onOpenChange,
    existingModelId,
    existingDisplayName,
    existingTrustAll,
    existingTrustedWorkspaces,
    existingFeishuEnabled,
    existingFeishuAppId,
    existingFeishuBaseUrl,
    initialValues,
    saveFeatureConfig,
    onComplete,
}) => {
    const [saving, setSaving] = useState(false);
    const initialValuesRef = useRef<Record<string, unknown>>(initialValues || {});

    const {
        state,
        setModelId,
        setDisplayName,
        checkBunVersion,
        checkUvVersion,
        installBun,
        installUv,
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
        existingTrustedWorkspaces,
        existingFeishuEnabled,
        existingFeishuAppId,
        existingFeishuBaseUrl,
    });

    const { models, loading: modelsLoading } = useModels(true);
    const modelOptions = useMemo(
        () =>
            models.map((model) => ({
                value: `${model.code}%%${model.llm_provider_id}`,
                label: model.name,
            })),
        [models]
    );

    useEffect(() => {
        if (open) {
            initialValuesRef.current = initialValues || {};
        }
    }, [initialValues, open]);

    const persistWizard = useCallback(async (closeAfterSave: boolean) => {
        setSaving(true);
        try {
            await saveExperimentalConfigValues(saveFeatureConfig, {
                ...initialValuesRef.current,
                butler_experiment_enabled: "true",
                butler_model_id: state.modelId,
                butler_display_name: state.displayName || "总管家",
                butler_trust_all_workspaces: String(state.trustAllWorkspaces),
                butler_trusted_workspaces: JSON.stringify(state.trustedWorkspaces),
                butler_feishu_enabled: String(state.feishuEnabled),
                butler_feishu_app_id: state.feishuAppId,
                butler_feishu_base_url: state.feishuBaseUrl,
            });
            initialValuesRef.current = {
                ...initialValuesRef.current,
                butler_experiment_enabled: "true",
                butler_model_id: state.modelId,
                butler_display_name: state.displayName || "总管家",
                butler_trust_all_workspaces: String(state.trustAllWorkspaces),
                butler_trusted_workspaces: JSON.stringify(state.trustedWorkspaces),
                butler_feishu_enabled: String(state.feishuEnabled),
                butler_feishu_app_id: state.feishuAppId,
                butler_feishu_base_url: state.feishuBaseUrl,
            };

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
        <Dialog open={open} onOpenChange={onOpenChange}>
            <DialogContent className="max-w-2xl max-h-[85vh] flex flex-col p-0 gap-0">
                <DialogHeader className="px-6 pt-6 pb-4 border-b border-border/50">
                    <div className="flex items-center justify-between">
                        <div>
                            <DialogTitle className="text-lg">总管家设置向导</DialogTitle>
                            <DialogDescription className="mt-1">
                                按步骤完成总管家的基本配置
                            </DialogDescription>
                        </div>
                    </div>

                    {/* Step indicator */}
                    <div className="flex items-center justify-center gap-1 pt-3">
                        {STEP_LABELS.map((label, index) => (
                            <React.Fragment key={index}>
                                {index > 0 && (
                                    <div
                                        className={`h-px w-8 transition-colors ${
                                            index <= state.currentStep
                                                ? "bg-primary"
                                                : "bg-border"
                                        }`}
                                    />
                                )}
                                <button
                                    type="button"
                                    onClick={() => {
                                        // Only allow going back or to completed steps
                                        if (index <= state.currentStep) {
                                            // Direct access via goToStep from hook
                                            // Use goPrev/goNext pattern
                                        }
                                    }}
                                    className={`flex items-center gap-1.5 px-2 py-1 rounded-md text-xs font-medium transition-colors ${
                                        index === state.currentStep
                                            ? "text-primary bg-primary/10"
                                            : index < state.currentStep
                                              ? "text-green-600 dark:text-green-400"
                                              : "text-muted-foreground"
                                    }`}
                                >
                                    {index < state.currentStep ? (
                                        <CheckCircle2 className="h-3.5 w-3.5" />
                                    ) : (
                                        <span
                                            className={`w-5 h-5 rounded-full text-[11px] flex items-center justify-center ${
                                                index === state.currentStep
                                                    ? "bg-primary text-primary-foreground"
                                                    : "bg-muted text-muted-foreground"
                                            }`}
                                        >
                                            {index + 1}
                                        </span>
                                    )}
                                    <span className="hidden sm:inline">{label}</span>
                                </button>
                            </React.Fragment>
                        ))}
                    </div>
                </DialogHeader>

                {/* Step content */}
                <div className="flex-1 overflow-y-auto px-6 py-5">
                    {state.currentStep === 0 && (
                        <StepModelConfig
                            modelId={state.modelId}
                            displayName={state.displayName}
                            modelOptions={modelOptions}
                            modelsLoading={modelsLoading}
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
                            trustedWorkspaces={state.trustedWorkspaces}
                            trustAllWorkspaces={state.trustAllWorkspaces}
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
                </div>

                {/* Footer navigation */}
                <div className="flex items-center justify-between px-6 py-4 border-t border-border/50 bg-muted/20">
                    <div>
                        {canGoPrev && (
                            <Button
                                variant="ghost"
                                onClick={goPrev}
                                disabled={saving}
                            >
                                <ChevronLeft className="h-4 w-4 mr-1" />
                                上一步
                            </Button>
                        )}
                    </div>
                    <div className="flex items-center gap-2">
                        {canGoNext && state.currentStep > 0 && (
                            <Button
                                variant="ghost"
                                onClick={() => void handleSkip()}
                                disabled={saving}
                            >
                                跳过
                            </Button>
                        )}
                        <Button
                            onClick={() => void handleNext()}
                            disabled={
                                (state.currentStep === 0 && !currentStepValid) || saving
                            }
                        >
                            {saving ? (
                                "保存中..."
                            ) : isLastStep ? (
                                "完成设置"
                            ) : (
                                <>
                                    下一步
                                    <ChevronRight className="h-4 w-4 ml-1" />
                                </>
                            )}
                        </Button>
                    </div>
                </div>
            </DialogContent>
        </Dialog>
    );
};

export default ButlerOnboardingWizard;
