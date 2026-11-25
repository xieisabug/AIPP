import React from "react";
import ButlerConfigCenter from "@/components/config/butler/ButlerConfigCenter";

interface ButlerPageConfigProps {
    subNav?: string;
    onSubNavConsumed?: () => void;
    navigateTo?: (menuKey: string, subNav?: string) => void;
}

export const ButlerPageConfig: React.FC<ButlerPageConfigProps> = ({
    subNav,
    onSubNavConsumed,
    navigateTo,
}) => {
    return (
        <ButlerConfigCenter
            enableLeaveGuard
            subNav={subNav}
            onSubNavConsumed={onSubNavConsumed}
            navigateTo={navigateTo}
        />
    );
};

export default ButlerPageConfig;
