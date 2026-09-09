import { ReactNode, CSSProperties } from 'react';

interface CircleButtonProps {
    primary?: boolean;
    size?: 'mini' | 'small' | 'medium' | 'large';
    icon: ReactNode;
    onClick: () => void;
    className?: string;
    type?: 'submit' | 'button';
    style?: CSSProperties;
    disabled?: boolean;
    title?: string;
    'aria-label'?: string;
    'aria-pressed'?: boolean;
    dataThemeSlot?: string;
    dataAippSlot?: string;
    dataState?: string;
    customVisual?: boolean;
}

const CircleButton: React.FC<CircleButtonProps> = ({
    primary,
    icon,
    type,
    onClick,
    className,
    size,
    style,
    disabled,
    title,
    'aria-label': ariaLabel,
    'aria-pressed': ariaPressed,
    dataThemeSlot,
    dataAippSlot,
    dataState,
    customVisual,
}) => {
    const sizeClasses = {
        mini: 'h-6 w-6 rounded-[12px]',
        small: 'h-8 w-8 rounded-2xl',
        medium: 'h-8 w-8 rounded-2xl',
        large: 'h-14 w-14 rounded-[28px]'
    };

    return <button 
        onClick={onClick} 
        className={`fixed border border-primary flex items-center justify-center cursor-pointer disabled:cursor-not-allowed disabled:opacity-55 ${primary && !customVisual ? 'border-0 bg-action' : ''} ${customVisual ? 'border-0 bg-transparent overflow-visible' : ''} ${sizeClasses[size || 'medium']} ${className || ''}`}
        type={type || 'button'}
        style={style}
        disabled={disabled}
        title={title}
        data-theme-slot={dataThemeSlot}
        data-aipp-slot={dataAippSlot}
        data-state={dataState}
        data-custom-visual={customVisual ? "true" : undefined}
        aria-label={ariaLabel}
        aria-pressed={ariaPressed}
    >
        {icon}
    </button>
}

export default CircleButton;
