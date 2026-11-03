/** Shares inline styles with index.html so loading stays centered before JS/CSS arrives. */
export default function LoadingScreen({ title = "正在准备你的 AI 助手" }: { title?: string }) {
    return (
        <div className="aipp-loading-screen" role="status" aria-live="polite">
            <div className="aipp-loading-spinner" aria-hidden="true" />
            <p className="aipp-loading-title">{title}</p>
            <p className="aipp-loading-description">
                {title === "正在准备你的 AI 助手" ? "请稍等，马上就能开始对话" : "请稍等，正在准备页面内容"}
            </p>
        </div>
    );
}
