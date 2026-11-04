package com.xieisabug.aipp

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.provider.OpenableColumns
import android.print.PrintManager
import android.util.Base64
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.result.ActivityResult
import androidx.core.content.FileProvider
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.ByteArrayOutputStream
import java.io.File
import java.util.UUID
import java.util.concurrent.Executors

@InvokeArg
class MobileFileArgs {
    lateinit var root: String
    var payload: JSObject = JSObject()
}

/** All external reads originate in a system picker, never a caller-supplied URI. */
@TauriPlugin
class MobileFilesPlugin(private val activity: Activity) : Plugin(activity) {
    private val worker = Executors.newSingleThreadExecutor()
    private val singleLimit = 20L * 1024 * 1024
    private val batchLimit = 50L * 1024 * 1024
    private val exportLimit = 32L * 1024 * 1024
    private var pickerBusy = false
    private var printView: WebView? = null
    private var pendingPrint: Invoke? = null
    private val mainHandler = android.os.Handler(android.os.Looper.getMainLooper())
    private val types = mapOf("png" to "image/png", "jpg" to "image/jpeg", "jpeg" to "image/jpeg",
        "gif" to "image/gif", "webp" to "image/webp", "txt" to "text/plain", "md" to "text/markdown",
        "csv" to "text/csv", "json" to "application/json", "zip" to "application/zip")

    private fun fail(invoke: Invoke, stage: String, ex: Exception) {
        invoke.reject("MOBILE_FILE $stage: ${ex.javaClass.simpleName}: ${ex.message ?: "native layer supplied no detail"}")
    }

    private fun args(invoke: Invoke): MobileFileArgs {
        val value = invoke.getArgs()
        return MobileFileArgs().apply {
            root = value.getString("root")
            payload = value.getJSObject("payload") ?: JSObject()
        }
    }

    private fun root(args: MobileFileArgs): File {
        val file = File(args.root).canonicalFile
        val base = activity.filesDir.canonicalPath
        require(file.path == base || file.path.startsWith(base + File.separator)) { "unauthorized application directory" }
        require(file.name == "mobile-files") { "invalid managed directory" }
        file.mkdirs()
        return file
    }

    @Command
    fun pick(invoke: Invoke) {
        if (pickerBusy) { invoke.reject("MOBILE_FILE picker: another document operation is pending"); return }
        try {
            val args = args(invoke)
            root(args)
            val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type = "*/*"
                putExtra(Intent.EXTRA_ALLOW_MULTIPLE, !args.payload.optBoolean("skill", false))
                putExtra(Intent.EXTRA_MIME_TYPES, if (args.payload.optBoolean("skill", false))
                    arrayOf("application/zip", "application/x-zip-compressed") else types.filterKeys { it != "zip" }.values.distinct().toTypedArray())
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            pickerBusy = true
            startActivityForResult(invoke, intent, "picked")
        } catch (ex: Exception) { pickerBusy = false; fail(invoke, "picker", ex) }
    }

    @ActivityCallback
    fun picked(invoke: Invoke, result: ActivityResult) {
        pickerBusy = false
        if (result.resultCode == Activity.RESULT_CANCELED) {
            invoke.resolve(JSObject().put("status", "cancelled").put("files", JSArray()))
            return
        }
        worker.execute {
            try {
                check(result.resultCode == Activity.RESULT_OK) { "picker result=${result.resultCode}" }
                val args = args(invoke)
                val dir = File(root(args), "imports").apply { mkdirs() }
                val data = requireNotNull(result.data) { "picker omitted result intent" }
                val uris = data.clipData?.let { clip -> (0 until clip.itemCount).map { clip.getItemAt(it).uri } }
                    ?: listOf(requireNotNull(data.data) { "picker omitted document URI" })
                require(uris.size <= 10) { "batch exceeds 10 documents" }
                val files = JSArray()
                val errors = JSArray()
                var total = 0L
                for (uri in uris) {
                    var target: File? = null
                    try {
                        require(uri.scheme == "content") { "picker did not provide content URI" }
                        val resolver = activity.contentResolver
                        var name = "document"
                        var declaredSize = -1L
                        resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { cursor ->
                            if (cursor.moveToFirst()) {
                                name = cursor.getString(0) ?: name
                                if (!cursor.isNull(1)) declaredSize = cursor.getLong(1)
                            }
                        }
                        name = name.substringAfterLast('/').substringAfterLast('\\').replace(Regex("[\\x00-\\x1f]"), "_").take(160)
                        val ext = name.substringAfterLast('.', "").lowercase()
                        val mime = types[ext] ?: error("unsupported format: $name; supported: PNG/JPEG/GIF/WebP, UTF-8 TXT/MD/CSV/JSON")
                        require((ext == "zip") == args.payload.optBoolean("skill", false)) { "ZIP is only accepted by Skills import" }
                        require(declaredSize <= singleLimit) { "$name exceeds 20 MiB" }
                        require(declaredSize < 0 || total + declaredSize <= batchLimit) { "batch exceeds 50 MiB" }
                        require(name.isNotBlank() && name != "." && name != "..") { "invalid document name" }
                        val documentDir = File(dir, UUID.randomUUID().toString())
                        check(documentDir.mkdir()) { "cannot create private document directory" }
                        target = File(documentDir, name)
                        var count = 0L
                        resolver.openInputStream(uri)?.use { input ->
                            target.outputStream().use { output ->
                                count = copyMobileFile(input, output, singleLimit, batchLimit, total) { dir.usableSpace }
                            }
                        } ?: error("ContentResolver returned no readable stream (grant/provider failure)")
                        var thumbnail: String? = null
                        if (mime.startsWith("image/")) {
                            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                            BitmapFactory.decodeFile(target.path, bounds)
                            require(bounds.outWidth > 0 && bounds.outHeight > 0) { "invalid image encoding" }
                            require(bounds.outMimeType == mime) { "image MIME does not match filename: expected $mime, decoded ${bounds.outMimeType}" }
                            val opts = BitmapFactory.Options().apply { inSampleSize = maxOf(1, maxOf(bounds.outWidth, bounds.outHeight) / 192) }
                            BitmapFactory.decodeFile(target.path, opts)?.let { bitmap ->
                                val out = ByteArrayOutputStream()
                                bitmap.compress(Bitmap.CompressFormat.PNG, 100, out)
                                bitmap.recycle()
                                thumbnail = "data:image/png;base64," + Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP)
                            }
                        }
                        val metadata = JSObject().put("name", name).put("mime", mime).put("size", count).put("thumbnail", thumbnail)
                        File("${target.path}.metadata.json").writeText(metadata.toString(), Charsets.UTF_8)
                        total += count
                        files.put(JSObject().put("path", target.path).put("name", name).put("mime", mime).put("size", count).put("thumbnail", thumbnail))
                    } catch (ex: Exception) {
                        target?.let { File("${it.path}.metadata.json").delete() }
                        target?.delete()
                        target?.parentFile?.delete()
                        errors.put("${ex.javaClass.simpleName}: ${ex.message}")
                    }
                }
                invoke.resolve(JSObject().put("status", "selected").put("files", files).put("errors", errors))
            } catch (ex: Exception) { fail(invoke, "import", ex) }
        }
    }

    private fun stageExport(args: MobileFileArgs): File {
        val payload = args.payload
        val name = payload.getString("name")
        require(name == File(name).name && !name.contains('\\') && name.length <= 180) { "invalid export name" }
        validateExportType(payload)
        val encoded = payload.getString("data")
        require(encoded.length <= exportLimit * 4 / 3 + 8) { "export exceeds 32 MiB" }
        val bytes = Base64.decode(encoded, Base64.DEFAULT)
        require(bytes.size <= exportLimit) { "export exceeds 32 MiB" }
        val dir = File(root(args), "exports").apply { mkdirs() }
        check(dir.usableSpace > bytes.size + 1024 * 1024) { "insufficient storage for export" }
        // Exports are retained 7 days. Clipboard content is stored separately and retained until replaced.
        dir.listFiles()?.filter { System.currentTimeMillis() - it.lastModified() > 7L * 24 * 3600 * 1000 }?.forEach { it.delete() }
        val file = File(dir, "${UUID.randomUUID()}-$name")
        try { file.writeBytes(bytes) } catch (ex: Exception) { file.delete(); throw ex }
        return file
    }

    private fun validateExportType(payload: JSObject) {
        val mime = mapOf("md" to "text/markdown", "txt" to "text/plain", "docx" to "application/vnd.openxmlformats-officedocument.wordprocessingml.document", "png" to "image/png", "jpg" to "image/jpeg", "jpeg" to "image/jpeg", "gif" to "image/gif", "webp" to "image/webp", "pdf" to "application/pdf")
        val name = payload.getString("name")
        val expected = mime[name.substringAfterLast('.', "").lowercase()]
        require(expected != null && expected == payload.getString("mime")) { "unsupported export extension or MIME: $name" }
    }

    @Command
    fun save(invoke: Invoke) {
        if (pickerBusy) { invoke.reject("MOBILE_FILE save-picker: another document operation is pending"); return }
        try {
            val args = args(invoke)
            // Validate before opening the picker; bytes are written only to the returned grant.
            validateExportType(args.payload)
            require(args.payload.getString("data").length <= exportLimit * 4 / 3 + 8) { "export exceeds 32 MiB" }
            pickerBusy = true
            startActivityForResult(invoke, Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type = args.payload.getString("mime")
                putExtra(Intent.EXTRA_TITLE, args.payload.getString("name"))
                addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }, "saved")
        } catch (ex: Exception) { pickerBusy = false; fail(invoke, "save-picker", ex) }
    }

    @ActivityCallback
    fun saved(invoke: Invoke, result: ActivityResult) {
        pickerBusy = false
        if (result.resultCode == Activity.RESULT_CANCELED) { invoke.resolve(JSObject().put("status", "cancelled")); return }
        worker.execute {
            val uri = result.data?.data
            try {
                check(result.resultCode == Activity.RESULT_OK && uri != null) { "save picker omitted authorized URI: result=${result.resultCode}" }
                val args = args(invoke)
                val bytes = Base64.decode(args.payload.getString("data"), Base64.DEFAULT)
                require(bytes.size <= exportLimit) { "export exceeds 32 MiB" }
                activity.contentResolver.openOutputStream(uri, "wt")?.use { it.write(bytes); it.flush() }
                    ?: error("ContentResolver returned no writable stream")
                invoke.resolve(JSObject().put("status", "saved").put("uri", uri.toString()))
            } catch (ex: Exception) {
                invoke.reject("MOBILE_FILE save-write: ${ex.javaClass.simpleName}: ${ex.message ?: "native layer supplied no detail"}; the selected document may contain partial data, please inspect it")
            }
        }
    }

    @Command
    fun share(invoke: Invoke) = output(invoke, false)

    @Command
    fun open(invoke: Invoke) = output(invoke, true)

    private fun output(invoke: Invoke, open: Boolean) {
        worker.execute {
            try {
                val args = args(invoke)
                val file = stageExport(args)
                val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", file)
                activity.runOnUiThread {
                    try {
                        val intent = Intent(if (open) Intent.ACTION_VIEW else Intent.ACTION_SEND).apply {
                            setDataAndType(if (open) uri else null, args.payload.getString("mime"))
                            if (!open) putExtra(Intent.EXTRA_STREAM, uri)
                            clipData = ClipData.newRawUri(file.name, uri)
                            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                        }
                        activity.startActivity(if (open) intent else Intent.createChooser(intent, "分享导出文件"))
                        invoke.resolve(JSObject().put("status", if (open) "opened" else "share_launched"))
                    } catch (ex: Exception) { fail(invoke, "external-app", ex) }
                }
            } catch (ex: Exception) { fail(invoke, "export-cache", ex) }
        }
    }

    @Command
    fun copyImage(invoke: Invoke) {
        worker.execute {
            try {
                val args = args(invoke)
                require(args.payload.getString("mime") == "image/png") { "clipboard only supports PNG" }
                val file = stageExport(args)
                val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                BitmapFactory.decodeFile(file.path, bounds)
                require(bounds.outWidth > 0 && bounds.outHeight > 0 && bounds.outMimeType == "image/png") { "invalid PNG" }
                val dir = File(root(args), "clipboard").apply { mkdirs() }
                val clipboardFile = File(dir, file.name)
                check(file.renameTo(clipboardFile)) { "cannot retain clipboard image" }
                val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", clipboardFile)
                activity.runOnUiThread {
                    try {
                        val manager = activity.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                        manager.setPrimaryClip(ClipData.newUri(activity.contentResolver, "AIPP 图片", uri))
                        // Keep old entries for 7 days too: another app may still be reading the previous URI.
                        dir.listFiles()?.filter { it != clipboardFile && System.currentTimeMillis() - it.lastModified() > 7L * 24 * 3600 * 1000 }?.forEach { it.delete() }
                        invoke.resolve(JSObject().put("status", "copied"))
                    } catch (ex: Exception) { fail(invoke, "clipboard", ex) }
                }
            } catch (ex: Exception) { fail(invoke, "clipboard-image", ex) }
        }
    }

    @Command
    fun print(invoke: Invoke) {
        activity.runOnUiThread {
            try {
                check(printView == null) { "a PDF print document is already active" }
                val args = args(invoke)
                val html = args.payload.getString("html")
                require(html.length <= 8 * 1024 * 1024) { "print HTML exceeds 8 MiB; export Markdown/Word or split conversation" }
                val view = WebView(activity)
                printView = view
                pendingPrint = invoke
                mainHandler.postDelayed({
                    if (pendingPrint === invoke) {
                        pendingPrint = null
                        if (printView === view) printView = null
                        view.destroy()
                        invoke.reject("MOBILE_FILE print: WebView did not finish loading within 30 seconds")
                    }
                }, 30_000)
                view.settings.javaScriptEnabled = false
                view.settings.allowFileAccess = false
                view.settings.allowContentAccess = false
                view.settings.blockNetworkLoads = true
                view.webViewClient = object : WebViewClient() {
                    override fun onPageFinished(webView: WebView, url: String) {
                        if (pendingPrint !== invoke) return
                        try {
                            val name = args.payload.optString("name", "AIPP")
                            val manager = activity.getSystemService(Context.PRINT_SERVICE) as PrintManager
                            val job = manager.print(name, webView.createPrintDocumentAdapter(name), null)
                            pendingPrint = null
                            invoke.resolve(JSObject().put("status", "print_launched"))
                            val handler = android.os.Handler(android.os.Looper.getMainLooper())
                            val monitor = object : Runnable {
                                override fun run() {
                                    if (printView !== webView) return
                                    if (job.isCompleted || job.isCancelled || job.isFailed) {
                                        webView.destroy(); printView = null
                                    } else handler.postDelayed(this, 1000)
                                }
                            }
                            handler.postDelayed(monitor, 1000)
                        } catch (ex: Exception) { pendingPrint = null; webView.destroy(); printView = null; fail(invoke, "print-launch", ex) }
                    }
                }
                view.loadDataWithBaseURL(null, html, "text/html", "UTF-8", null)
            } catch (ex: Exception) {
                if (pendingPrint === invoke) { pendingPrint = null; printView?.destroy(); printView = null }
                fail(invoke, "print", ex)
            }
        }
    }

    override fun onDestroy(activity: androidx.appcompat.app.AppCompatActivity) {
        mainHandler.post {
            pendingPrint?.reject("MOBILE_FILE print: activity destroyed before print dialog opened")
            pendingPrint = null
            printView?.destroy()
            printView = null
        }
        worker.shutdown()
    }
}
