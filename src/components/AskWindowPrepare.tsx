import React, { useEffect, useState } from "react";
import { Badge } from "./ui/badge";

interface AskWindowPrepareProps {
    selectedText: string;
    isMobile?: boolean;
}

const AskWindowPrepare: React.FC<AskWindowPrepareProps> = ({
    selectedText,
    isMobile = false,
}) => {
    const [currentDate, setCurrentDate] = useState(
        new Date().toLocaleDateString(),
    );
    const [currentTime, setCurrentTime] = useState(
        new Date().toLocaleTimeString(),
    );

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

    return (
        <div className="text-xs text-foreground select-none" {...dragProps}>
            <p {...dragProps}>输入文本后回车，与快捷对话助手进行交流</p>
            <p {...dragProps}>
                拖拽或者粘贴文件/图片后，可与快捷对话助手根据文件进行交流
            </p>
            <p {...dragProps}>对话中可以使用以下!bang命令：</p>
            <div className="mt-2.5" {...dragProps}>
                <div className="mt-2.5 flex items-center" {...dragProps}>
                    <Badge className="mr-2 py-1.25 px-2.5 bg-primary rounded-lg text-primary-foreground flex-none cursor-pointer">!s</Badge>
                    <span className="text-foreground" {...dragProps}>插入选择的文字</span>
                    {selectedText && (
                        <span className="ml-2.5 bg-muted text-muted-foreground px-1 py-0.5 rounded max-w-96 overflow-hidden text-ellipsis whitespace-nowrap" {...dragProps}>
                            {selectedText}
                        </span>
                    )}
                </div>
                <div className="mt-2.5 flex items-center" {...dragProps}>
                    <Badge className="mr-2 py-1.25 px-2.5 bg-primary rounded-lg text-primary-foreground flex-none cursor-pointer">!cd</Badge>
                    <span className="text-foreground" {...dragProps}>插入当前日期文本</span>
                    <span className="ml-2.5 bg-muted text-muted-foreground px-1 py-0.5 rounded max-w-96 overflow-hidden text-ellipsis whitespace-nowrap" {...dragProps}>
                        {currentDate}
                    </span>
                </div>
                <div className="mt-2.5 flex items-center" {...dragProps}>
                    <Badge className="mr-2 py-1.25 px-2.5 bg-primary rounded-lg text-primary-foreground flex-none cursor-pointer">!ct</Badge>
                    <span className="text-foreground" {...dragProps}>插入当前时间文字</span>
                    <span className="ml-2.5 bg-muted text-muted-foreground px-1 py-0.5 rounded max-w-96 overflow-hidden text-ellipsis whitespace-nowrap" {...dragProps}>
                        {currentTime}
                    </span>
                </div>
                <div className="mt-2.5 flex items-center" {...dragProps}>
                    <Badge className="mr-2 py-1.25 px-2.5 bg-primary rounded-lg text-primary-foreground flex-none cursor-pointer">!w(url)</Badge>
                    <span className="text-foreground" {...dragProps}>插入网页内容</span>
                </div>
                <div className="mt-2.5 flex items-center" {...dragProps}>
                    <Badge className="mr-2 py-1.25 px-2.5 bg-primary rounded-lg text-primary-foreground flex-none cursor-pointer">!wm(url)</Badge>
                    <span className="text-foreground" {...dragProps}>
                        插入网页内容并转换为Markdown
                    </span>
                </div>
            </div>
        </div>
    );
};

export default AskWindowPrepare;
