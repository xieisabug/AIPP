import React from "react";
import { UseFormReturn } from "react-hook-form";
import { DisplayConfigForm } from "./forms/DisplayConfigForm";
import { SummaryConfigForm } from "./forms/SummaryConfigForm";
import { PreviewConfigForm } from "./forms/PreviewConfigForm";
import { NetworkConfigForm } from "./forms/NetworkConfigForm";
import { DataFolderConfigForm } from "./forms/DataFolderConfigForm";
import { ShortcutsConfigForm } from "./forms/ShortcutsConfigForm";

interface FeatureItem {
    id: string;
    name: string;
    description: string;
    icon: React.ReactNode;
    code: string;
}

interface FeatureFormRendererProps {
    selectedFeature: FeatureItem;
    forms: {
        displayForm: UseFormReturn<any>;
        summaryForm: UseFormReturn<any>;
        previewForm: UseFormReturn<any>;
        networkForm: UseFormReturn<any>;
        dataFolderForm: UseFormReturn<any>;
        shortcutsForm: UseFormReturn<any>;
    };
    versionManager: {
        bunVersion: string;
        uvVersion: string;
        isInstallingBun: boolean;
        isInstallingUv: boolean;
        bunInstallLog: string;
        uvInstallLog: string;
        installBun: () => void;
        installUv: () => void;
        bunLatestVersion: string | null;
        uvLatestVersion: string | null;
        isCheckingBunUpdate: boolean;
        isCheckingUvUpdate: boolean;
        isUpdatingBun: boolean;
        isUpdatingUv: boolean;
        checkBunUpdate: (useProxy: boolean) => void;
        checkUvUpdate: (useProxy: boolean) => void;
        updateBun: (useProxy: boolean) => void;
        updateUv: (useProxy: boolean) => void;
    };
    onSaveDisplay: () => Promise<void>;
    onSaveSummary: () => Promise<void>;
    onSaveNetwork: () => Promise<void>;
    onSaveShortcuts: () => Promise<void>;
}

export const FeatureFormRenderer: React.FC<FeatureFormRendererProps> = ({
    selectedFeature,
    forms,
    versionManager,
    onSaveDisplay,
    onSaveSummary,
    onSaveNetwork,
    onSaveShortcuts,
}) => {
    switch (selectedFeature.id) {
        case "display":
            return (
                <DisplayConfigForm
                    form={forms.displayForm}
                    onSave={onSaveDisplay}
                />
            );
        case "conversation_summary":
            return (
                <SummaryConfigForm
                    form={forms.summaryForm}
                    onSave={onSaveSummary}
                />
            );
        case "preview":
            return (
                <PreviewConfigForm
                    form={forms.previewForm}
                    bunVersion={versionManager.bunVersion}
                    uvVersion={versionManager.uvVersion}
                    isInstallingBun={versionManager.isInstallingBun}
                    isInstallingUv={versionManager.isInstallingUv}
                    bunInstallLog={versionManager.bunInstallLog}
                    uvInstallLog={versionManager.uvInstallLog}
                    onInstallBun={versionManager.installBun}
                    onInstallUv={versionManager.installUv}
                    bunLatestVersion={versionManager.bunLatestVersion}
                    uvLatestVersion={versionManager.uvLatestVersion}
                    isCheckingBunUpdate={versionManager.isCheckingBunUpdate}
                    isCheckingUvUpdate={versionManager.isCheckingUvUpdate}
                    isUpdatingBun={versionManager.isUpdatingBun}
                    isUpdatingUv={versionManager.isUpdatingUv}
                    checkBunUpdate={versionManager.checkBunUpdate}
                    checkUvUpdate={versionManager.checkUvUpdate}
                    updateBun={versionManager.updateBun}
                    updateUv={versionManager.updateUv}
                />
            );
        case "data_folder":
            return (
                <DataFolderConfigForm
                    form={forms.dataFolderForm}
                />
            );
        case "network_config":
            return (
                <NetworkConfigForm
                    form={forms.networkForm}
                    onSave={onSaveNetwork}
                />
            );
        case "shortcuts":
            return (
                <ShortcutsConfigForm
                    form={forms.shortcutsForm}
                    onSave={onSaveShortcuts}
                />
            );
        default:
            return null;
    }
};