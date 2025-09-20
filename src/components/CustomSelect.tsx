import React, { useState, useRef, useEffect } from "react";
import { ChevronDown, Check } from "lucide-react";

interface Option {
    value: string;
    label: string;
    // 可选的静态图标（emoji、dataURL、http(s) URL）
    icon?: string;
    // 可选的组件图标（如 lucide-react 的组件），仅前端使用
    iconComponent?: React.ReactNode | React.ComponentType<{ className?: string; size?: number }>;
}

interface CustomSelectProps {
    options: Option[];
    value: string;
    onChange: (value: string) => void;
    placeholder?: string;
    className?: string;
}

const CustomSelect: React.FC<CustomSelectProps> = ({
    options,
    value,
    onChange,
    placeholder = "请选择...",
    className = "",
}) => {
    const [isOpen, setIsOpen] = useState<boolean>(false);
    const selectRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        const handleClickOutside = (event: MouseEvent) => {
            if (selectRef.current && !selectRef.current.contains(event.target as Node)) {
                setIsOpen(false);
            }
        };

        document.addEventListener("mousedown", handleClickOutside);
        return () => {
            document.removeEventListener("mousedown", handleClickOutside);
        };
    }, []);

    const handleSelectClick = () => {
        setIsOpen(!isOpen);
    };

    const handleOptionClick = (optionValue: string) => {
        onChange(optionValue);
        setIsOpen(false);
    };

    const selectedOption = options.find((option) => option.value === value);

    const renderIcon = (opt?: Option) => {
        if (!opt) return null;
        const size = 16;
        if (opt.iconComponent) {
            // 如果是组件类型，支持传入组件或节点
            if (React.isValidElement(opt.iconComponent)) {
                return <span className="mr-2 inline-flex items-center justify-center">{opt.iconComponent}</span>;
            }
            const Comp = opt.iconComponent as React.ComponentType<{ className?: string; size?: number }>;
            return <Comp className="mr-2" size={size} />;
        }
        if (opt.icon) {
            // 简单判断是否是URL/dataURL，否则按emoji字符渲染
            const isUrl = /^data:|^https?:\/\//i.test(opt.icon);
            if (isUrl) {
                return (
                    <img
                        src={opt.icon}
                        alt="icon"
                        className="mr-2 h-4 w-4 object-contain inline-block align-middle"
                        onError={(e) => {
                            (e.currentTarget as HTMLImageElement).style.display = "none";
                        }}
                    />
                );
            }
            return <span className="mr-2 text-base leading-none">{opt.icon}</span>;
        }
        return null;
    };

    return (
        <div className={`relative ${className}`} ref={selectRef}>
            <button
                type="button"
                className={`
          w-full flex items-center justify-between px-3 py-2 
          bg-background border border-border rounded-lg shadow-sm
          text-left text-sm text-foreground
          hover:border-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary focus:border-primary
          transition-colors duration-200
          ${isOpen ? "border-primary ring-2 ring-primary" : ""}
        `}
                onClick={handleSelectClick}
                title={selectedOption?.label || placeholder}
            >
                <span className="flex items-center gap-2 truncate">
                    {renderIcon(selectedOption)}
                    <span className="truncate">{selectedOption?.label || placeholder}</span>
                </span>
                <ChevronDown
                    className={`
            h-4 w-4 text-muted-foreground transition-transform duration-200
            ${isOpen ? "rotate-180" : ""}
          `}
                />
            </button>

            {isOpen && (
                <div className="absolute z-50 w-full mt-1 bg-background border border-border rounded-lg shadow-lg max-h-60 overflow-auto">
                    {options.map((option) => (
                        <div
                            key={option.value}
                            className={`
                flex items-center justify-between px-3 py-2 cursor-pointer text-sm
                hover:bg-muted hover:text-foreground
                ${option.value === value ? "bg-muted text-foreground" : "text-foreground"}
              `}
                            onClick={() => handleOptionClick(option.value)}
                            title={option.label}
                        >
                            <span className="flex items-center gap-2 truncate">
                                {renderIcon(option)}
                                <span className="truncate">{option.label}</span>
                            </span>
                            {option.value === value && (
                                <Check className="h-4 w-4 text-muted-foreground flex-shrink-0" />
                            )}
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
};

export default CustomSelect;
