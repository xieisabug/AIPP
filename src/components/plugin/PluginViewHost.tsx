import React, { useMemo } from "react";
import { AlertCircle } from "lucide-react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import type { LoadedPlugin } from "@/services/PluginRuntime";

interface PluginViewHostProps {
    pluginList: LoadedPlugin[];
    location: string;
    context?: Record<string, unknown>;
    emptyDescription?: string;
}

interface RenderableView {
    plugin: LoadedPlugin;
    viewId: string;
    title: string;
    description?: string | null;
}

class PluginViewBoundary extends React.Component<{ children: React.ReactNode; pluginCode: string; instance: unknown }, { error: string | null }> {
    state: { error: string | null } = { error: null };
    static getDerivedStateFromError(error: unknown) {
        return { error: error instanceof Error ? error.message : String(error) };
    }
    componentDidUpdate(previous: Readonly<{ children: React.ReactNode; pluginCode: string; instance: unknown }>) {
        if (previous.instance !== this.props.instance && this.state.error) this.setState({ error: null });
    }
    render() {
        return this.state.error ? <p role="alert" className="break-words text-sm text-destructive">插件 {this.props.pluginCode} 界面执行失败：{this.state.error}</p> : this.props.children;
    }
}

const PluginViewHost: React.FC<PluginViewHostProps> = ({
    pluginList,
    location,
    context,
    emptyDescription = "当前没有插件在此位置提供界面。",
}) => {
    const views = useMemo<RenderableView[]>(() => {
        const normalizedLocation = location.trim();
        return pluginList.flatMap((plugin) => {
            const contributedViews = plugin.contributions?.views ?? [];
            return contributedViews
                .filter((view) => view.location === normalizedLocation)
                .map((view) => ({
                    plugin,
                    viewId: view.id,
                    title: view.title,
                    description: view.description,
                }));
        });
    }, [location, pluginList]);

    if (views.length === 0) {
        return (
            <div className="rounded-lg border border-dashed border-border p-4 text-sm text-muted-foreground">
                {emptyDescription}
            </div>
        );
    }

    return (
        <div className="space-y-4">
            {views.map(({ plugin, viewId, title, description }) => {
                const instance = plugin.instance as
                    | {
                        renderView?: (targetViewId: string, viewContext?: Record<string, unknown>) => React.ReactNode;
                    }
                    | null;
                let content: React.ReactNode = null;
                let errorText: string | null = null;

                try {
                    if (typeof instance?.renderView === "function") {
                        content = instance.renderView(viewId, context);
                    } else {
                        errorText = "插件未实现 renderView()。";
                    }
                } catch (error) {
                    errorText = error instanceof Error ? error.message : String(error);
                }

                return (
                    <Card key={`${plugin.code}:${viewId}`} className="shadow-none">
                        <CardHeader className="pb-3">
                            <CardTitle className="text-base">{title}</CardTitle>
                            <CardDescription>
                                {description || `${plugin.name} 提供的插件视图`}
                            </CardDescription>
                        </CardHeader>
                        <CardContent>
                            {errorText ? (
                                <div className="flex items-start gap-2 rounded-md border border-dashed border-destructive/40 p-3 text-sm text-destructive">
                                    <AlertCircle className="mt-0.5 h-4 w-4 flex-shrink-0" />
                                    <span>{errorText}</span>
                                </div>
                            ) : (
                                <PluginViewBoundary pluginCode={plugin.code} instance={instance}>{content}</PluginViewBoundary>
                            )}
                        </CardContent>
                    </Card>
                );
            })}
        </div>
    );
};

export default PluginViewHost;
