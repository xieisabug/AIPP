import React from "react";
import { motion } from "motion/react";
import { SubTaskExecutionSummary, useSubTaskIcon } from "../../data/SubTask";
import { getStatusIcon } from "../../services/subTaskService";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "../ui/tooltip";
import { ShineBorder } from "../magicui/shine-border";
import { DEFAULT_SHINE_BORDER_CONFIG } from "@/utils/shineConfig";

export interface SubTaskItemProps {
    execution: SubTaskExecutionSummary;
    onViewDetail?: (execution: SubTaskExecutionSummary) => void;
}

const SubTaskItem: React.FC<SubTaskItemProps> = ({
    execution,
    onViewDetail,
}) => {
    const iconComp = useSubTaskIcon(execution.task_code);
    const handleClick = () => {
        if (onViewDetail) {
            onViewDetail(execution);
        }
    };

    return (
        <TooltipProvider>
            <Tooltip>
                <TooltipTrigger asChild>
                    <motion.div
                        layout
                        initial={{ opacity: 0, scale: 0.9 }}
                        animate={{ opacity: 1, scale: 1 }}
                        exit={{ opacity: 0, scale: 0.9 }}
                        transition={{ type: "spring", stiffness: 420, damping: 30, mass: 0.6 }}
                        whileHover={{ scale: 1.06 }}
                        whileTap={{ scale: 0.98 }}
                        className={
                            "inline-flex items-center justify-center w-8 h-8 rounded-full bg-white border-2 cursor-pointer relative select-none " +
                            "transition-all duration-300 ease-out " +
                            (execution.status === "running" ? " w-12 h-12" : "") +
                            (execution.status === "success"
                                ? " border-green-800"
                                : execution.status === "failed"
                                    ? " border-red-800"
                                    : " border-border")
                        }
                        onClick={handleClick}
                        aria-label={execution.task_name}
                        role="button"
                    >
                        {
                            execution.status === "running" && <ShineBorder
                                shineColor={DEFAULT_SHINE_BORDER_CONFIG.shineColor}
                                borderWidth={DEFAULT_SHINE_BORDER_CONFIG.borderWidth}
                                duration={DEFAULT_SHINE_BORDER_CONFIG.duration}
                            />
                        }
                        {/* Status icon / Task icon */}
                        <motion.div
                            className={
                                "flex items-center justify-center transition-colors duration-300 " +
                                (execution.status === "success"
                                    ? " text-green-800"
                                    : execution.status === "failed"
                                        ? " text-red-800"
                                        : " text-foreground")
                            }
                            animate={{
                                scale: execution.status === "running" ? 1.05 : 1,
                                rotate: execution.status === "failed" ? [0, -2, 2, 0] : 0,
                            }}
                            transition={{
                                type: "spring",
                                stiffness: 420,
                                damping: 32,
                                mass: 0.5,
                            }}
                        >
                            {(() => {
                                if (!iconComp) {
                                    return getStatusIcon(execution.status);
                                }
                                const size = execution.status === "running" ? 20 : 16;
                                if (React.isValidElement(iconComp)) {
                                    return (
                                        <span className="inline-flex items-center justify-center" style={{ lineHeight: 0 }}>
                                            {iconComp}
                                        </span>
                                    );
                                }
                                const Comp = iconComp as React.ComponentType<{ className?: string; size?: number }>;
                                return <Comp className="text-current" size={size} />;
                            })()}
                        </motion.div>
                    </motion.div>
                </TooltipTrigger>

                <TooltipContent side="bottom" className="max-w-xs">
                    <div className="space-y-1 text-sm">
                        <div className="font-medium flex items-center gap-2">
                            <span>{execution.task_name}</span>
                        </div>
                        <div className="text-muted-foreground text-xs">
                            状态: {execution.status}
                        </div>
                        {execution.task_prompt && (
                            <div className="text-muted-foreground text-xs line-clamp-2">
                                提示: {execution.task_prompt}
                            </div>
                        )}
                        <div className="text-muted-foreground text-xs">
                            创建时间: {execution.created_time.toLocaleTimeString()}
                        </div>
                        {execution.token_count > 0 && (
                            <div className="text-muted-foreground text-xs">
                                Token: {execution.token_count}
                            </div>
                        )}
                    </div>
                </TooltipContent>
            </Tooltip>
        </TooltipProvider>
    );
};

export default React.memo(SubTaskItem);
