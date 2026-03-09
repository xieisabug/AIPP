import { ReactNode, MouseEventHandler, forwardRef } from "react";

interface IconButtonProps {
    icon: ReactNode;
    onClick: MouseEventHandler<HTMLButtonElement>;
    className?: string;
    border?: boolean;
    type?: "button" | "submit" | "reset";
    dataAippSlot?: string;
}

const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(
    ({ icon, onClick, className, border, type = "button", dataAippSlot }, ref) => {
        return (
            <button
                ref={ref}
                type={type}
                onClick={onClick}
                className={`h-8 w-8 rounded-2xl border-0 flex items-center justify-center cursor-pointer ${
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
