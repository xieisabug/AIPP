import { ChevronRight } from "lucide-react";
import type { ReactNode } from "react";

/** Keeps fields mounted so collapsing a section never discards form values. */
export default function OptionalConfigSection({ title, description, children }: {
    title: string;
    description?: string;
    children: ReactNode;
}) {
    return <details className="group lg:col-span-2 border-t border-border pt-2">
        <summary className="flex min-h-11 cursor-pointer list-none items-center gap-2 text-sm font-medium [&::-webkit-details-marker]:hidden">
            <ChevronRight className="h-4 w-4 text-muted-foreground group-open:rotate-90" />
            {title}
        </summary>
        {description && <p className="mb-6 text-sm text-muted-foreground">{description}</p>}
        <div className="space-y-6">{children}</div>
    </details>;
}
