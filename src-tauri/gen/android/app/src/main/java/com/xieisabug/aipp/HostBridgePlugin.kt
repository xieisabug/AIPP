package com.xieisabug.aipp

import android.app.Activity
import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.journeyapps.barcodescanner.ScanOptions
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

@TauriPlugin
class HostBridgePlugin(private val activity: Activity) : Plugin(activity) {
    private val prefs = activity.getSharedPreferences("aipp-paired-hosts", Context.MODE_PRIVATE)
    private var scanning = false

    @Command
    fun scan(invoke: Invoke) {
        if (scanning) { invoke.reject("扫码器已打开"); return }
        scanning = true
        try {
            val intent = ScanOptions().setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                .setPrompt("扫描电脑上 AIPP 的设备互联二维码").setBeepEnabled(false)
                .createScanIntent(activity)
            startActivityForResult(invoke, intent, "scanned")
        } catch (e: Exception) { scanning = false; invoke.reject("打开扫码器失败：${e.message}") }
    }

    @ActivityCallback
    fun scanned(invoke: Invoke, result: ActivityResult) {
        scanning = false
        if (result.resultCode == Activity.RESULT_CANCELED) { invoke.resolve(JSObject()); return }
        val code = result.data?.getStringExtra("SCAN_RESULT")
        if (code.isNullOrBlank()) { invoke.reject("扫码器没有返回二维码内容"); return }
        invoke.resolve(JSObject().put("code", code))
    }

    private fun key(create: Boolean): SecretKey {
        val alias = "aipp.host_bridge.v1"
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(alias, null) as? SecretKey)?.let { return it }
        check(create) { "设备配对密钥已丢失，请重新配对" }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build())
        }.generateKey()
    }

    @Command
    @Synchronized
    fun savePair(invoke: Invoke) {
        try {
            val args = invoke.getArgs()
            val id = args.getString("id")
            val endpoint = requireNotNull(args.getJSObject("endpoint"))
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, key(true))
            cipher.updateAAD(id.toByteArray())
            val sealed = cipher.doFinal(args.getString("token").toByteArray())
            val record = JSObject().put("endpoint", endpoint)
                .put("iv", Base64.encodeToString(cipher.iv, Base64.NO_WRAP))
                .put("sealed", Base64.encodeToString(sealed, Base64.NO_WRAP))
            check(prefs.edit().putString(id, record.toString()).commit()) { "写入设备存储失败" }
            invoke.resolve()
        } catch (e: Exception) { invoke.reject("保存电脑配对失败：${e.message}") }
    }

    @Command
    fun environments(invoke: Invoke) {
        try {
            val hosts = JSArray()
            for ((id, raw) in prefs.all.toSortedMap()) {
                val record = org.json.JSONObject(raw as String)
                hosts.put(JSObject().put("id",id).put("name",record.getJSONObject("endpoint").getString("name")))
            }
            invoke.resolve(JSObject().put("hosts",hosts))
        } catch (e: Exception) { invoke.reject("读取已配对电脑失败：${e.message}") }
    }

    fun readPair(id: String): String {
        return try {
            val record = org.json.JSONObject(requireNotNull(prefs.getString(id,null)) { "电脑尚未配对" })
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, key(false), GCMParameterSpec(128, Base64.decode(record.getString("iv"), Base64.NO_WRAP)))
            cipher.updateAAD(id.toByteArray())
            val token = cipher.doFinal(Base64.decode(record.getString("sealed"), Base64.NO_WRAP)).decodeToString()
            JSObject().put("endpoint",record.getJSONObject("endpoint")).put("token",token).toString()
        } catch (e: Exception) { JSObject().put("error", "访问电脑配对凭据失败：${e.message}").toString() }
    }
}
