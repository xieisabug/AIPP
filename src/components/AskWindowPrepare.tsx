import React, { useEffect, useState } from "react";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "./ui/collapsible";
import { ChevronDown } from "lucide-react";

interface AskWindowPrepareProps {
    selectedText: string;
    /** 实际移动平台标志，不使用桌面窗口的窄屏状态。 */
    isMobile?: boolean;
    /** 无模型提供商/未完成首次配置时展示引导入口 */
    showSetupGuides?: boolean;
    onConfigureProviders?: () => void;
    onChooseAssistant?: () => void;
}

const AskWindowPrepare: React.FC<AskWindowPrepareProps> = ({
    selectedText,
    isMobile = false,
    showSetupGuides = false,
    onConfigureProviders,
    onChooseAssistant,
}) => {
    const [currentDate, setCurrentDate] = useState(
        new Date().toLocaleDateString(),
    );
    const [currentTime, setCurrentTime] = useState(
        new Date().toLocaleTimeString(),
    );
    const [helpOpen, setHelpOpen] = useState(false);

    useEffect(() => {
        const intervalId = setInterval(() => {
            setCurrentTime(new Date().toLocaleTimeString());
        }, 1000);

        return () => clearInterval(intervalId);
    }, []);

    useEffect(() => {
        const intervalId = setInterval(() => {
            setCurrentDate(new Date().toLocaleDateString());
        }, 60000);

        return () => clearInterval(intervalId);
    }, []);

    // 移动端不需要拖动区域
    const dragProps = isMobile ? {} : { "data-tauri-drag-region": true };

    const helpContent = (
        <>
            <p {...dragProps}>
                拖拽或者粘贴文件/图片后，可与快捷对话助手根据文件进行交流
            </p>
            {isMobile ? (
                <p {...dragProps}>需要换行时可用输入框内换行；发送请点发送按钮。</p>
            ) : null}
            <p {...dragProps}>对话中可以使用以下!bang命令：</p>
            <div className="mt-2.5" data-aipp-slot="ask-prepare-bang-list" {...dragProps}>
                <div className="mt-2.5 flex items-center" data-aipp-slot="ask-prepare-bang-item" {...dragProps}>
                    <Badge className="mr-2 py-1.25 px-2.5 bg-primary rounded-lg text-primary-foreground flex-none cursor-pointer">!s</Badge>
                    <span className="text-foreground" {...dragProps}>插入选择的文字</span>
                    {selectedText && (
                        <span className="ml-2.5 bg-muted text-muted-foreground px-1 py-0.5 rounded max-w-96 overflow-hidden text-ellipsis whitespace-nowrap" {...dragProps}>
                            {selectedText}
                        </span>
                    )}
                </div>
                <div className="mt-2.5 flex items-center" data-aipp-slot="ask-prepare-bang-item" {...dragProps}>
                    <Badge className="mr-2 py-1.25 px-2.5 bg-primary rounded-lg text-primary-foreground flex-none cursor-pointer">!cd</Badge>
                    <span className="text-foreground" {...dragProps}>插入当前日期文本</span>
                    <span className="ml-2.5 bg-muted text-muted-foreground px-1 py-0.5 rounded max-w-96 overflow-hidden text-ellipsis whitespace-nowrap" {...dragProps}>
                        {currentDate}
                    </span>
                </div>
                <div className="mt-2.5 flex items-center" data-aipp-slot="ask-prepare-bang-item" {...dragProps}>
                    <Badge className="mr-2 py-1.25 px-2.5 bg-primary rounded-lg text-primary-foreground flex-none cursor-pointer">!ct</Badge>
                    <span className="text-foreground" {...dragProps}>插入当前时间文字</span>
                    <span className="ml-2.5 bg-muted text-muted-foreground px-1 py-0.5 rounded max-w-96 overflow-hidden text-ellipsis whitespace-nowrap" {...dragProps}>
                        {currentTime}
                    </span>
                </div>
                <div className="mt-2.5 flex items-center" data-aipp-slot="ask-prepare-bang-item" {...dragProps}>
                    <Badge className="mr-2 py-1.25 px-2.5 bg-primary rounded-lg text-primary-foreground flex-none cursor-pointer">!w(url)</Badge>
                    <span className="text-foreground" {...dragProps}>插入网页内容</span>
                </div>
                <div className="mt-2.5 flex items-center" data-aipp-slot="ask-prepare-bang-item" {...dragProps}>
                    <Badge className="mr-2 py-1.25 px-2.5 bg-primary rounded-lg text-primary-foreground flex-none cursor-pointer">!wm(url)</Badge>
                    <span className="text-foreground" {...dragProps}>
                        插入网页内容并转换为Markdown
                    </span>
                </div>
                <div className="mt-2.5 flex items-center" data-aipp-slot="ask-prepare-bang-item" {...dragProps}>
                    <Badge className="mr-2 py-1.25 px-2.5 bg-primary rounded-lg text-primary-foreground flex-none cursor-pointer">!file(path)</Badge>
                    <span className="text-foreground" {...dragProps}>
                        读取文本文件内容
                    </span>
                </div>
            </div>
        </>
    );

    return (
        <div className="text-xs text-foreground select-none" data-aipp-slot="ask-prepare-content" {...dragProps}>
            <p className={isMobile ? "text-sm text-foreground" : undefined} {...dragProps}>
                {isMobile
                    ? "输入内容后点击发送，开始与助手交流"
                    : "输入文本后回车，与快捷对话助手进行交流"}
            </p>

            {showSetupGuides ? (
                <div className="mt-4 flex flex-col items-center gap-2" data-aipp-slot="ask-prepare-setup-guides">
                    <p className="text-muted-foreground">首次使用请先完成配置</p>
                    <div className="flex flex-wrap justify-center gap-2">
                        {onConfigureProviders ? (
                            <Button type="button" size="sm" className="min-h-11" onClick={onConfigureProviders}>
                                配置模型提供商
                            </Button>
                        ) : null}
                        {onChooseAssistant ? (
                            <Button type="button" size="sm" variant="outline" className="min-h-11" onClick={onChooseAssistant}>
                                选择助手
                            </Button>
                        ) : null}
                    </div>
                </div>
            ) : null}

            {isMobile ? (
                <Collapsible open={helpOpen} onOpenChange={setHelpOpen} className="mt-4 text-left">
                    <CollapsibleTrigger asChild>
                        <button
                            type="button"
                            className="inline-flex min-h-11 items-center gap-1 text-muted-foreground hover:text-foreground"
                            data-aipp-slot="ask-prepare-help-toggle"
                        >
                            <ChevronDown className={`h-3.5 w-3.5 transition-transform ${helpOpen ? "rotate-180" : ""}`} />
                            高级提示（拖拽 / 回车 / !bang）
                        </button>
                    </CollapsibleTrigger>
                    <CollapsibleContent className="mt-2 space-y-2">
                        {helpContent}
                    </CollapsibleContent>
                </Collapsible>
            ) : helpContent}
        </div>
    );
};

export default AskWindowPrepare;
