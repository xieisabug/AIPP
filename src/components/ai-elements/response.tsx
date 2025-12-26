"use client";

import UnifiedMarkdown from "@/components/UnifiedMarkdown";
import { cn } from "@/utils/utils";
import { type ComponentProps, memo } from "react";

type ResponseProps = ComponentProps<typeof UnifiedMarkdown>;

export const Response = memo(
    ({ className, ...props }: ResponseProps) => (
        <UnifiedMarkdown
            className={cn(
                "size-full [&>*:first-child]:mt-0 [&>*:last-child]:mb-0",
                className
            )}
            {...props}
        />
    ),
    (prevProps, nextProps) => prevProps.children === nextProps.children
);

Response.displayName = "Response";
