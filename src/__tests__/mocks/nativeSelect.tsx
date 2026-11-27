/**
 * `@/components/ui/select` 的测试替身：把 Radix Select 渲染成原生 `<select>`，
 * 让以表单逻辑为主的测试继续用 `fireEvent.change(select, { target: { value } })` 驱动选择。
 * 用法：`vi.mock("@/components/ui/select", () => import("@/__tests__/mocks/nativeSelect"));`
 * 只覆盖受控/非受控取值、占位、分组、禁用；下拉弹层本身的交互请用真组件测试。
 */
import React from "react";

type Children = { children?: React.ReactNode };

export function SelectTrigger(_: Children & Record<string, unknown>) {
    return null;
}

export function SelectValue(_: { placeholder?: React.ReactNode }) {
    return null;
}

export function SelectLabel(_: Children) {
    return null;
}

export function SelectSeparator() {
    return null;
}

export function SelectContent({ children }: Children) {
    return <>{children}</>;
}

export function SelectGroup({ children }: Children) {
    let label: React.ReactNode;
    const rest: React.ReactNode[] = [];
    React.Children.forEach(children, (child) => {
        if (React.isValidElement<Children>(child) && child.type === SelectLabel) label = child.props.children;
        else rest.push(child);
    });
    return <optgroup label={typeof label === "string" ? label : undefined}>{rest}</optgroup>;
}

export function SelectItem({ value, disabled, children }: Children & { value: string; disabled?: boolean }) {
    return <option value={value} disabled={disabled}>{children}</option>;
}

export function Select({ value, defaultValue, onValueChange, disabled, required, name, children }: Children & {
    value?: string;
    defaultValue?: string;
    onValueChange?: (value: string) => void;
    disabled?: boolean;
    required?: boolean;
    name?: string;
}) {
    let trigger: Record<string, unknown> = {};
    const rest: React.ReactNode[] = [];
    React.Children.forEach(children, (child) => {
        if (React.isValidElement<Record<string, unknown>>(child) && child.type === SelectTrigger) trigger = child.props;
        else rest.push(child);
    });
    const props: Record<string, unknown> = { ...trigger };
    const triggerChildren = props.children;
    const triggerDisabled = props.disabled;
    for (const key of ["children", "className", "size", "disabled"]) delete props[key];
    let placeholder: React.ReactNode;
    React.Children.forEach(triggerChildren as React.ReactNode, (child) => {
        if (React.isValidElement<{ placeholder?: React.ReactNode }>(child) && child.type === SelectValue) {
            placeholder = child.props.placeholder;
        }
    });
    return (
        <select
            {...props}
            name={name}
            required={required}
            disabled={disabled || Boolean(triggerDisabled)}
            {...(value !== undefined ? { value } : { defaultValue: defaultValue ?? "" })}
            onChange={(event) => onValueChange?.(event.target.value)}
        >
            {placeholder !== undefined && <option value="" disabled>{placeholder}</option>}
            {rest}
        </select>
    );
}
