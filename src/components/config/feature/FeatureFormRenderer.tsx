import React from "react";
import { UseFormReturn } from "react-hook-form";
import { DisplayConfigForm } from "./forms/DisplayConfigForm";
import { SummaryConfigForm } from "./forms/SummaryConfigForm";
import { PreviewConfigForm } from "./forms/PreviewConfigForm";
import { NetworkConfigForm } from "./forms/NetworkConfigForm";
import { DataFolderConfigForm } from "./forms/DataFolderConfigForm";
import { ShortcutsConfigForm } from "./forms/ShortcutsConfigForm";
import { OtherConfigForm } from "./forms/OtherConfigForm";
import { AboutConfigForm } from "./forms/AboutConfigForm";
import { ExperimentalConfigForm } from "./forms/ExperimentalConfigForm";

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
        otherForm: UseFormReturn<any>;
        experimentalForm: UseFormReturn<any>;
        aboutForm: UseFormReturn<any>;
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
        // Python 相关
        python2Version: string;
        python3Version: string;
        installedPythons: string[];
        needInstallPython3: boolean;
        isInstallingPython: boolean;
        pythonInstallLog: string;
        checkPythonVersions: () => void;
        installPython3: () => void;
    };
    onSaveDisplay: () => Promise<void>;
    onSaveSummary: () => Promise<void>;
    onSaveNetwork: () => Promise<void>;
    onSaveShortcuts: () => Promise<void>;
    onSaveExperimental: () => Promise<void>;
}

export const FeatureFormRenderer: React.FC<FeatureFormRendererProps> = ({
    selectedFeature,
    forms,
    versionManager,
    onSaveDisplay,
    onSaveSummary,
    onSaveNetwork,
    onSaveShortcuts,
    onSaveExperimental,
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
                    // Python 相关
                    python2Version={versionManager.python2Version}
                    python3Version={versionManager.python3Version}
                    installedPythons={versionManager.installedPythons}
                    needInstallPython3={versionManager.needInstallPython3}
                    isInstallingPython={versionManager.isInstallingPython}
                    pythonInstallLog={versionManager.pythonInstallLog}
                    checkPythonVersions={versionManager.checkPythonVersions}
                    installPython3={versionManager.installPython3}
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
        case "other":
            return (
                <OtherConfigForm
                    form={forms.otherForm}
                />
            );
        case "experimental":
            return (
                <ExperimentalConfigForm
                    form={forms.experimentalForm}
                    onSave={onSaveExperimental}
                />
            );
        case "about":
            return (
                <AboutConfigForm
                    form={forms.aboutForm}
                />
            );
        default:
            return null;
    }
};
