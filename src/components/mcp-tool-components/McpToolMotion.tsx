import React from "react";
import { AnimatePresence, motion, useReducedMotion, type HTMLMotionProps } from "motion/react";
import { cn } from "@/utils/utils";

type DivProps = React.HTMLAttributes<HTMLDivElement>;

const TOOL_MOTION_DURATION_SECONDS = 0.24;
const TOOL_MOTION_EASE = [0.16, 1, 0.3, 1] as const;

const cardTransition = {
    duration: TOOL_MOTION_DURATION_SECONDS,
    ease: TOOL_MOTION_EASE,
} as const;

const contentTransition = {
    duration: TOOL_MOTION_DURATION_SECONDS,
    ease: TOOL_MOTION_EASE,
} as const;

const getMotionScale = (shouldReduceMotion: boolean | null) => (
    shouldReduceMotion ? 0.98 : 0.86
);

const getMetricScale = (shouldReduceMotion: boolean | null) => (
    shouldReduceMotion ? 0.98 : 0.88
);

const getMotionOffset = (shouldReduceMotion: boolean | null) => (
    shouldReduceMotion ? 1 : 6
);

const getMotionBlur = (shouldReduceMotion: boolean | null) => (
    shouldReduceMotion ? "blur(0px)" : "blur(4px)"
);

interface MotionToolCardProps extends DivProps {
    interactive?: boolean;
}

export const MotionToolCard = React.forwardRef<HTMLDivElement, MotionToolCardProps>(
    ({ className, interactive = false, children, ...props }, ref) => {
        const shouldReduceMotion = useReducedMotion();
        const baseClassName = cn(
            "w-full max-w-[600px] my-1 p-2 border border-border rounded-md bg-card overflow-hidden relative transition-colors",
            interactive && "cursor-pointer hover:border-primary/40 hover:bg-muted/30",
            className,
        );

        return (
            <motion.div
                ref={ref}
                className={baseClassName}
                initial={{ opacity: 0, y: shouldReduceMotion ? 1 : 4 }}
                animate={{ opacity: 1, y: 0 }}
                transition={cardTransition}
                data-mcp-motion="card"
                {...(props as HTMLMotionProps<"div">)}
            >
                {children}
            </motion.div>
        );
    },
);

MotionToolCard.displayName = "MotionToolCard";

interface MotionStatusSlotProps {
    stateKey: React.Key;
    present?: boolean;
    children: React.ReactNode;
}

export const MotionStatusSlot: React.FC<MotionStatusSlotProps> = ({ stateKey, present = true, children }) => {
    const shouldReduceMotion = useReducedMotion();

    if (!present) {
        return null;
    }

    return (
        <AnimatePresence mode="popLayout">
            <motion.div
                key={stateKey}
                className="flex items-center"
                initial={{
                    opacity: 0,
                    y: getMotionOffset(shouldReduceMotion),
                    scale: getMotionScale(shouldReduceMotion),
                    filter: getMotionBlur(shouldReduceMotion),
                }}
                animate={{ opacity: 1, y: 0, scale: 1, filter: "blur(0px)" }}
                exit={{ opacity: 0, y: shouldReduceMotion ? -1 : -4, scale: 0.96, filter: shouldReduceMotion ? "blur(0px)" : "blur(2px)" }}
                transition={contentTransition}
                data-mcp-motion="status"
            >
                {children}
            </motion.div>
        </AnimatePresence>
    );
};

interface MotionPresenceProps {
    show: boolean;
    children: React.ReactNode;
    className?: string;
}

export const MotionDetails: React.FC<MotionPresenceProps> = ({ show, children, className }) => {
    const shouldReduceMotion = useReducedMotion();

    return (
        <AnimatePresence>
            {show && (
                <motion.div
                    className={className}
                    initial={{
                        height: 0,
                        opacity: 0,
                        y: getMotionOffset(shouldReduceMotion),
                        filter: getMotionBlur(shouldReduceMotion),
                    }}
                    animate={{ height: "auto", opacity: 1, y: 0, filter: "blur(0px)" }}
                    exit={{ height: 0, opacity: 0, y: shouldReduceMotion ? -1 : -4, filter: shouldReduceMotion ? "blur(0px)" : "blur(2px)" }}
                    transition={contentTransition}
                    style={{ overflow: "hidden" }}
                    data-mcp-motion="details"
                >
                    {children}
                </motion.div>
            )}
        </AnimatePresence>
    );
};

export const MotionMetaRow: React.FC<MotionPresenceProps> = ({ show, children, className }) => {
    const shouldReduceMotion = useReducedMotion();

    return (
        <AnimatePresence>
            {show && (
                <motion.div
                    className={className}
                    initial={{
                        opacity: 0,
                        y: getMotionOffset(shouldReduceMotion),
                        filter: getMotionBlur(shouldReduceMotion),
                    }}
                    animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
                    exit={{ opacity: 0, y: shouldReduceMotion ? -1 : -4, filter: shouldReduceMotion ? "blur(0px)" : "blur(2px)" }}
                    transition={contentTransition}
                    data-mcp-motion="meta-row"
                >
                    {children}
                </motion.div>
            )}
        </AnimatePresence>
    );
};

interface MotionMetricItemProps {
    metricKey: React.Key;
    children: React.ReactNode;
}

export const MotionMetricItem: React.FC<MotionMetricItemProps> = ({ metricKey, children }) => {
    const shouldReduceMotion = useReducedMotion();

    return (
        <AnimatePresence mode="popLayout">
            <motion.span
                key={metricKey}
                className="inline-flex"
                initial={{
                    opacity: 0,
                    y: getMotionOffset(shouldReduceMotion),
                    scale: getMetricScale(shouldReduceMotion),
                    filter: getMotionBlur(shouldReduceMotion),
                }}
                animate={{ opacity: 1, y: 0, scale: 1, filter: "blur(0px)" }}
                exit={{ opacity: 0, y: shouldReduceMotion ? -1 : -4, scale: 0.97, filter: shouldReduceMotion ? "blur(0px)" : "blur(2px)" }}
                transition={contentTransition}
                data-mcp-motion="metric"
            >
                {children}
            </motion.span>
        </AnimatePresence>
    );
};
