package com.xieisabug.aipp

import android.os.Bundle
import android.webkit.WebView
import android.view.View
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import android.util.Log
import java.util.concurrent.Executors

class MainActivity : TauriActivity() {
  override val handleBackNavigation: Boolean = false
  private var appWebView: WebView? = null
  private var backPending = false
  private val ipcExecutor = Executors.newSingleThreadExecutor { task ->
    Thread(task, "AippIpc").apply { isDaemon = true }
  }
  private external fun initCertificateVerifier()

  override fun onWebViewCreate(webView: WebView) {
    appWebView = webView
    initCertificateVerifier()
    super.onWebViewCreate(webView)
  }

  override fun setContentView(view: View?) {
    // Wry calls onWebViewCreate BEFORE adding its legacy Java IPC interface.
    // setContentView follows that registration, while its initial loadUrl is
    // still queued on the UI thread. Replace the bridge at this final boundary.
    if (view is RustWebView) installFrameAwareIpc(view)
    super.setContentView(view)
  }

  private fun installFrameAwareIpc(webView: RustWebView) {
    // addJavascriptInterface is exposed to every frame, including sandboxed
    // srcdoc. Use the frame-aware WebMessage API before the first navigation.
    check(WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
      "AIPP IPC initialization failed: Android System WebView lacks WEB_MESSAGE_LISTENER; update System WebView"
    }
    webView.removeJavascriptInterface("ipc")
    WebViewCompat.addWebMessageListener(webView, "ipc", setOf("*")) { view, message, sourceOrigin, isMainFrame, _ ->
      val host = sourceOrigin.host.orEmpty()
      val trustedHost = host == "tauri.localhost" || host == "localhost" || host == "127.0.0.1"
      val developmentHost = BuildConfig.DEBUG && (host.startsWith("192.168.") || host.startsWith("10.") ||
        Regex("^172\\.(1[6-9]|2[0-9]|3[01])\\.").containsMatchIn(host))
      if (!isMainFrame || sourceOrigin.scheme !in listOf("http", "https") || (!trustedHost && !developmentHost)) {
        Log.w("AippSecurity", "Rejected IPC from non-main or untrusted frame: $sourceOrigin")
        return@addWebMessageListener
      }
      val body = message.data ?: return@addWebMessageListener
      val nativeView = view as RustWebView
      val id = nativeView.id
      val url = view.url ?: sourceOrigin.toString()
      ipcExecutor.execute { Rust.ipc(id, url, body) }
    }
  }
  override fun onResume() {
    super.onResume()
    // Re-sync icon contrast after rotation / process foreground (#151).
    SystemUiPlugin.reapply(this)
  }

  override fun onDestroy() {
    ipcExecutor.shutdown()
    appWebView = null
    super.onDestroy()
  }
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // Register before the asynchronously created WebView is ready, so an early
    // Back cannot destroy the Activity while the native runtime is starting.
    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        // App navigation uses React state, not WebView history. Going back in
        // bootstrap history can load tauri.localhost/ and render "Not Found".
        // Keep the Activity and WebView alive with the native runtime.
        val view = appWebView
        if (ViewCompat.getRootWindowInsets(window.decorView)?.isVisible(WindowInsetsCompat.Type.ime()) == true) {
          WindowInsetsControllerCompat(window, window.decorView).hide(WindowInsetsCompat.Type.ime())
          return
        }
        if (backPending) return
        if (view == null) { moveTaskToBack(true); return }
        backPending = true
        view.evaluateJavascript("window.__aippMobileBack ? window.__aippMobileBack() : false") { handled ->
          backPending = false
          if (handled != "true") moveTaskToBack(true)
        }
      }
    })
  }
}
