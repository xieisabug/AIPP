import React, { memo } from 'react';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";
import { Search } from "lucide-react";

interface SidebarListProps {
    title: string;
    description: string;
    icon: React.ReactNode;
    children: React.ReactNode;
    addButton?: React.ReactNode;
    searchValue?: string;
    onSearchChange?: (value: string) => void;
    searchPlaceholder?: string;
}

const SidebarList: React.FC<SidebarListProps> = ({
    title,
    description,
    icon,
    children,
    addButton,
    searchValue,
    onSearchChange,
    searchPlaceholder = "搜索..."
}) => {
    return (
        <Card className="bg-gradient-to-br from-muted/20 to-muted/40 border-border h-fit sticky top-6 shadow-none">
            <CardHeader className="pb-3">
                <div className="flex items-start justify-between">
                    <div className="flex-1 min-w-0">
                        <CardTitle className="text-lg font-semibold text-foreground flex items-center gap-2">
                            {icon}
                            {title}
                        </CardTitle>
                        <CardDescription className="text-muted-foreground mt-2">
                            {description}
                        </CardDescription>
                    </div>
                    {addButton && (
                        <div className="flex-shrink-0 ml-3">
                            {addButton}
                        </div>
                    )}
                </div>
                {onSearchChange !== undefined && (
                    <div className="relative mt-3">
                        <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground" />
                        <Input
                            value={searchValue ?? ""}
                            onChange={(e) => onSearchChange(e.target.value)}
                            placeholder={searchPlaceholder}
                            className="pl-8 h-8 text-sm"
                        />
                    </div>
                )}
            </CardHeader>
            <CardContent className="space-y-3">
                {children}
            </CardContent>
        </Card>
    );
};

export default memo(SidebarList); 