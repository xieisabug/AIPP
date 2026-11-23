// 比选预览框自报内容高度：宿主（src/components/choices/FitFrame.tsx）据此伸缩 iframe。
// 由 document.rs 追加到预览文档、serve-build.mjs 追加到构建产物页面、前端追加到 srcdoc。
// 只向父窗口发送一个数字；父窗口校验来源窗口，并用 CSS 限制最大高度。
(() => {
    let last = -1;
    let queued = false;
    const report = () => {
        queued = false;
        const root = document.documentElement;
        // 标准模式下 <html> 的高度就是内容高度，可随内容变矮；
        // 页面把 html/body 设为 100% 时改看溢出的滚动高度。
        const overflow = root.scrollHeight > root.clientHeight ? root.scrollHeight : 0;
        const height = Math.ceil(Math.max(root.getBoundingClientRect().height, overflow));
        if (Math.abs(height - last) < 2) return;
        last = height;
        parent.postMessage({ type: "aipp-frame-height", height }, "*");
    };
    const schedule = () => {
        if (queued) return;
        queued = true;
        requestAnimationFrame(report);
    };
    const resize = new ResizeObserver(schedule);
    resize.observe(document.documentElement);
    if (document.body) resize.observe(document.body);
    new MutationObserver(schedule).observe(document.documentElement, { childList: true, subtree: true });
    addEventListener("load", schedule);
    schedule();
})();
