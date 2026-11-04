import { ReactNode, MouseEventHandler, forwardRef } from "react";

interface IconButtonProps {
    icon: ReactNode;
    onClick: MouseEventHandler<HTMLButtonElement>;
    className?: string;
    border?: boolean;
    type?: "button" | "submit" | "reset";
    dataAippSlot?: string;
    disabled?: boolean;
    title?: string;
    /** 无障碍名称；缺省时回退到 title */
    ariaLabel?: string;
}

const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(
    ({ icon, onClick, className, border, type = "button", dataAippSlot, disabled = false, title, ariaLabel }, ref) => {
        return (
            <button
                ref={ref}
                type={type}
                onClick={onClick}
                disabled={disabled}
                title={title}
                aria-label={ariaLabel ?? title}
                className={`h-8 w-8 rounded-2xl border-0 flex items-center justify-center ${
                    disabled ? "cursor-not-allowed opacity-50" : "cursor-pointer"
                } ${
                    border ? "border border-secondary bg-primary-foreground hover:border-primary" : ""
                } ${className || ""}`}
                data-aipp-slot={dataAippSlot}
            >
                {icon}
            </button>
        );
    }
);

IconButton.displayName = "IconButton";

export default IconButton;
