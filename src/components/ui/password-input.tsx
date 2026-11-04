import * as React from "react";
import { Eye, EyeOff } from "lucide-react";

import { cn } from "@/utils/utils";
import { Input } from "./input";

/** Masked input with a reveal toggle. Extra props (id, aria-*, value, onChange, ref) go to the input. */
function PasswordInput({ className, ...props }: Omit<React.ComponentProps<"input">, "type">) {
    const [visible, setVisible] = React.useState(false);
    return (
        <div className="relative w-full">
            <Input {...props} type={visible ? "text" : "password"} className={cn("pr-11", className)} />
            <button
                type="button"
                className="absolute right-0 top-1/2 flex h-11 w-11 -translate-y-1/2 items-center justify-center text-muted-foreground hover:text-foreground disabled:pointer-events-none"
                aria-label={visible ? "隐藏" : "显示"}
                aria-pressed={visible}
                disabled={props.disabled}
                onClick={() => setVisible((value) => !value)}
            >
                {visible ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
            </button>
        </div>
    );
}

export { PasswordInput };
