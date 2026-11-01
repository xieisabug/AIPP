package com.xieisabug.aipp

import android.app.Activity
import android.content.Context
import androidx.core.view.WindowInsetsControllerCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class SystemUiAppearanceArgs {
    /** Resolved app theme: "light" or "dark". */
    var theme: String = "light"
}

/**
 * Sync native status/navigation bar icon appearance with the DOM theme (#151).
 * Light content → dark icons (appearanceLight=true); dark content → light icons.
 */
@TauriPlugin
class SystemUiPlugin(private val activity: Activity) : Plugin(activity) {
    companion object {
        private const val PREFS = "aipp_system_ui"
        private const val KEY_LIGHT_ICONS = "light_status_icons"

        fun apply(activity: Activity, lightIcons: Boolean) {
            activity.runOnUiThread {
                val controller = WindowInsetsControllerCompat(activity.window, activity.window.decorView)
                controller.isAppearanceLightStatusBars = lightIcons
                controller.isAppearanceLightNavigationBars = lightIcons
                activity.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                    .edit()
                    .putBoolean(KEY_LIGHT_ICONS, lightIcons)
                    .apply()
            }
        }

        fun reapply(activity: Activity) {
            val lightIcons = activity
                .getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                .getBoolean(KEY_LIGHT_ICONS, true)
            apply(activity, lightIcons)
        }
    }

    @Command
    fun setStatusBarAppearance(invoke: Invoke) {
        val args = invoke.parseArgs(SystemUiAppearanceArgs::class.java)
        val lightIcons = args.theme.lowercase() != "dark"
        apply(activity, lightIcons)
        invoke.resolve()
    }
}
