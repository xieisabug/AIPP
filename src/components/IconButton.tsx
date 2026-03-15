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
}

const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(
    ({ icon, onClick, className, border, type = "button", dataAippSlot, disabled = false, title }, ref) => {
        return (
            <button
                ref={ref}
                type={type}
                onClick={onClick}
                disabled={disabled}
                title={title}
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
