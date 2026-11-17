//! 关闭 WebView 自带的常规自动填充（Edge 的「已保存的信息」下拉）。
//!
//! 应用里的输入框（提供商、助手配置、聊天等）都不是网页表单，浏览器按字段名猜出的
//! 姓名/地址建议只会干扰输入；Chromium 会对它判断为姓名/地址的字段忽略
//! `autocomplete="off"`，所以在 WebView2 设置层面统一关闭。每个 webview 创建时生效。
use tauri::Runtime;

pub fn init<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("webview-autofill")
        .on_webview_ready(|webview| {
            #[cfg(windows)]
            disable_general_autofill(&webview);
            #[cfg(not(windows))]
            let _ = webview;
        })
        .build()
}

#[cfg(windows)]
fn disable_general_autofill<R: Runtime>(webview: &tauri::Webview<R>) {
    use tracing::warn;
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings4;
    use windows_core::Interface;

    let label = webview.label().to_string();
    let result = webview.with_webview(move |platform| {
        // SAFETY: WebView2 COM 调用在 with_webview 回调里（webview 所在线程）执行，
        // controller 由 Tauri 持有且在回调期间有效。
        let outcome = unsafe {
            platform
                .controller()
                .CoreWebView2()
                .and_then(|core| core.Settings())
                .and_then(|settings| settings.cast::<ICoreWebView2Settings4>())
                .and_then(|settings| settings.SetIsGeneralAutofillEnabled(false))
        };
        if let Err(error) = outcome {
            warn!(webview = %label, %error, "关闭 WebView2 常规自动填充失败（ICoreWebView2Settings4 不可用或设置调用出错）");
        }
    });
    if let Err(error) = result {
        warn!(webview = %webview.label(), %error, "无法访问 WebView2 实例，未能关闭常规自动填充");
    }
}
