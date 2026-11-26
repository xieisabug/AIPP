import React, { useEffect, useMemo, useRef, useState } from "react";
import { useOnboardingEntry } from "@/components/onboarding/OnboardingSetup";
import { Search, X } from "lucide-react";
import { Input } from "@/components/ui/input";
import { usePinyinReady } from "@/utils/pinyinFilter";
import { searchSettings, type SettingsSearchItem } from "@/lib/settingsSearchIndex";
import type { SettingsMenuId } from "@/lib/settingsNavigation";

interface SettingsSearchProps {
    isMobilePlatform: boolean;
    availableMenus: Set<string>;
    onSelect: (menu: SettingsMenuId, subNav?: string) => void;
    compact?: boolean;
}

export const SettingsSearch: React.FC<SettingsSearchProps> = ({
    isMobilePlatform,
    availableMenus,
    onSelect,
    compact = false,
}) => {
    const [query, setQuery] = useState("");
    const [open, setOpen] = useState(false);
    const [activeIndex, setActiveIndex] = useState(0);
    const containerRef = useRef<HTMLDivElement>(null);
    const inputRef = useRef<HTMLInputElement>(null);
    const pinyinReady = usePinyinReady(query.trim().length > 0);

    const canOpen = useOnboardingEntry(!isMobilePlatform);
    const results = useMemo(
        () => searchSettings(query, { isMobilePlatform, availableMenus }).filter(item => item.id !== "first-run-onboarding" || canOpen),
        [query, isMobilePlatform, availableMenus, pinyinReady, canOpen]
    );

    useEffect(() => {
        setActiveIndex(0);
    }, [query]);

    // Ctrl/Cmd + K focuses settings search
    useEffect(() => {
        if (isMobilePlatform) return;
        const onKeyDown = (event: KeyboardEvent) => {
            if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
                event.preventDefault();
                inputRef.current?.focus();
                inputRef.current?.select();
                setOpen(true);
            }
        };
        window.addEventListener("keydown", onKeyDown);
        return () => window.removeEventListener("keydown", onKeyDown);
    }, [isMobilePlatform]);

    useEffect(() => {
        const handleOutside = (event: MouseEvent) => {
            if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
                setOpen(false);
            }
        };
        document.addEventListener("mousedown", handleOutside);
        return () => document.removeEventListener("mousedown", handleOutside);
    }, []);

    const handlePick = (item: SettingsSearchItem) => {
        setOpen(false);
        setQuery("");
        onSelect(item.menu, item.subNav);
    };

    const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
        if (event.key === "Escape") {
            setOpen(false);
            if (query) setQuery("");
            return;
        }
        if (!results.length) return;
        if (event.key === "ArrowDown") {
            event.preventDefault();
            setActiveIndex((prev) => (prev + 1) % results.length);
        } else if (event.key === "ArrowUp") {
            event.preventDefault();
            setActiveIndex((prev) => (prev - 1 + results.length) % results.length);
        } else if (event.key === "Enter") {
            event.preventDefault();
            const picked = results[activeIndex] ?? results[0];
            if (picked) handlePick(picked);
        }
    };

    const showDropdown = open && query.trim().length > 0;

    return (
        <div ref={containerRef} className="relative" data-aipp-slot="settings-search">
            <div className="relative">
                <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
                <Input
                    ref={inputRef}
                    type="text"
                    value={query}
                    onFocus={() => setOpen(true)}
                    onChange={(event) => {
                        setQuery(event.target.value);
                        setOpen(true);
                    }}
                    onKeyDown={handleKeyDown}
                    placeholder={isMobilePlatform ? "搜索设置..." : "搜索设置… (Ctrl+K)"}
                    aria-label="搜索设置"
                    className={`${compact ? "h-9" : "h-8"} bg-background pl-8 pr-7 text-xs`}
                />
                {query ? (
                    <button
                        type="button"
                        aria-label="清空搜索"
                        onClick={() => {
                            setQuery("");
                            inputRef.current?.focus();
                        }}
                        className="aipp-icon-button absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-0.5 text-muted-foreground hover:text-foreground"
                    >
                        <X className="h-3.5 w-3.5" />
                    </button>
                ) : null}
            </div>

            {showDropdown ? (
                <div
                    role="listbox"
                    aria-label="设置搜索结果"
                    className="absolute left-0 right-0 top-full z-50 mt-1.5 max-h-80 overflow-y-auto rounded-lg border border-border bg-popover p-1 shadow-lg"
                >
                    {results.length === 0 ? (
                        <div className="px-3 py-4 text-center text-xs text-muted-foreground">
                            未找到匹配的设置项
                        </div>
                    ) : (
                        results.map((item, idx) => (
                            <button
                                key={item.id}
                                type="button"
                                role="option"
                                aria-selected={idx === activeIndex}
                                onMouseEnter={() => setActiveIndex(idx)}
                                onClick={() => handlePick(item)}
                                className={`flex w-full flex-col gap-0.5 rounded-md px-2.5 py-2 text-left transition-colors ${
                                    idx === activeIndex
                                        ? "bg-accent text-accent-foreground"
                                        : "hover:bg-muted/60"
                                }`}
                            >
                                <div className="flex items-center justify-between gap-2">
                                    <span className="truncate text-xs font-medium">{item.title}</span>
                                    <span className="shrink-0 rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                                        {item.section}
                                    </span>
                                </div>
                                <span className="line-clamp-1 text-[11px] text-muted-foreground">
                                    {item.description}
                                </span>
                            </button>
                        ))
                    )}
                </div>
            ) : null}
        </div>
    );
};

export default SettingsSearch;
