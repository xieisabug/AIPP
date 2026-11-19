import { createContext, useContext, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { cn } from "@/utils/utils";

const FooterTarget = createContext<HTMLDivElement | null>(null);

/** Keep the active detail form's actions beside its scrolling body. */
export function ConfigFooterLayout({ children }: { children: ReactNode }) {
    const [target, setTarget] = useState<HTMLDivElement | null>(null);
    return <FooterTarget.Provider value={target}>
        {children}
        <div ref={setTarget} className="max-h-[45%] shrink-0 overflow-y-auto" />
    </FooterTarget.Provider>;
}

export function ConfigSheetBody({ children }: { children: ReactNode }) {
    return <ConfigFooterLayout>
        <div className="min-h-0 flex-1 overflow-y-auto">{children}</div>
    </ConfigFooterLayout>;
}

export function ConfigSaveFooter({ children }: { children: ReactNode }) {
    const target = useContext(FooterTarget);
    const footer = <div
        data-aipp-slot="config-form-save-footer"
        className={cn("[&_button]:h-11", target
            ? "border-t border-border bg-card px-4 pt-3 pb-[max(.75rem,env(safe-area-inset-bottom,0px))]"
            : "sticky bottom-0 z-20 mt-6 border-t border-border bg-card py-3")}
    >
        {children}
    </div>;
    return target ? createPortal(footer, target) : footer;
}
