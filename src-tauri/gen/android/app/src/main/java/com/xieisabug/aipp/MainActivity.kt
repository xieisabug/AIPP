package com.xieisabug.aipp

import android.os.Bundle
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  private external fun initCertificateVerifier()

  override fun onWebViewCreate(webView: WebView) {
    initCertificateVerifier()
    super.onWebViewCreate(webView)
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
        moveTaskToBack(true)
      }
    })
  }
}
