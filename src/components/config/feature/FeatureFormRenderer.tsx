import React from "react";
import { UseFormReturn } from "react-hook-form";
import { DisplayConfigForm } from "./forms/DisplayConfigForm";
import { SummaryConfigForm } from "./forms/SummaryConfigForm";
import { PreviewConfigForm } from "./forms/PreviewConfigForm";
import { NetworkConfigForm } from "./forms/NetworkConfigForm";
import { DataStorageConfigForm } from "./forms/DataStorageConfigForm";
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
                />
            );
        case "data_folder":
            return <DataStorageConfigForm />;
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