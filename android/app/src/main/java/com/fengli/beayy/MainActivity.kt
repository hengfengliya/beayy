package com.fengli.beayy

import android.annotation.SuppressLint
import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.view.WindowInsets
import android.widget.FrameLayout
import android.graphics.Color
import android.webkit.JavascriptInterface
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import org.json.JSONObject
import java.time.Instant
import java.util.concurrent.Executors

/**
 * 手机的界面就是电脑那套网页（app/ui，build.sh 放进 assets/ui）。网页调 window.__TAURI__.core.invoke(命令, 参数)：
 * android.js 把它转到这里的 invoke；普通命令交给 Rust 核心（和电脑同一份代码），native_ 开头的是只有手机有的（使用情况权限、读一轮）。
 */
class MainActivity : Activity() {
    private lateinit var web: WebView
    private val pool = Executors.newCachedThreadPool()

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        I18n.init(this)
        WebView.setWebContentsDebuggingEnabled(true)
        web = WebView(this)
        web.settings.javaScriptEnabled = true
        web.settings.domStorageEnabled = true
        web.settings.allowFileAccess = true
        web.webChromeClient = WebChromeClient()
        web.webViewClient = object : WebViewClient() {
            // 网页里的外链（帮助、GitHub）交给浏览器
            override fun shouldOverrideUrlLoading(view: WebView, req: WebResourceRequest): Boolean {
                val u = req.url
                if (u.scheme == "file") return false
                startActivity(Intent(Intent.ACTION_VIEW, u))
                return true
            }
        }
        web.addJavascriptInterface(Bridge(), "beayyAndroid")
        // 安卓 15 起页面铺到状态栏、导航条底下；网页里的 env(safe-area-inset-*) 在安卓上是 0，套一层留出来（WebView 自己不认 padding）
        val root = FrameLayout(this)
        root.setBackgroundColor(Color.WHITE)
        root.addView(web)
        if (Build.VERSION.SDK_INT >= 30) root.setOnApplyWindowInsetsListener { v, insets ->
            val b = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
            v.setPadding(b.left, b.top, b.right, b.bottom)
            WindowInsets.CONSUMED
        }
        setContentView(root)
        pool.execute { Core.start(applicationContext) }
        UsageJob.schedule(this)
        web.loadUrl("file:///android_asset/ui/index.html")
    }

    override fun onResume() {
        super.onResume()
        // 回到前台读一轮（从系统设置允许了权限回来也会走这里）
        pool.execute { Usage.read(applicationContext) }
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        if (web.canGoBack()) web.goBack() else super.onBackPressed()
    }

    inner class Bridge {
        @JavascriptInterface
        fun invoke(id: Int, cmd: String, args: String) {
            pool.execute {
                val out = try {
                    if (cmd.startsWith("native_")) JSONObject().put("ok", native(cmd, JSONObject(args))).toString()
                    else { Core.start(applicationContext); Core.call(cmd, args) }
                } catch (x: Exception) {
                    JSONObject().put("err", x.message ?: x.toString()).toString()
                }
                runOnUiThread { web.evaluateJavascript("window.__beayyResolve($id, $out)", null) }
            }
        }
    }

    private fun native(cmd: String, args: JSONObject): Any = when (cmd) {
        "native_status" -> JSONObject()
            .put("android", true)
            .put("usage", Usage.granted(this))
            .put("running", Usage.running)
            .put("logged_in", loggedIn())
            .apply {
                if (Usage.lastRead > 0) put("last_read", Instant.ofEpochMilli(Usage.lastRead).toString())
                Usage.lastError?.let { put("last_error", it) }
            }
        "native_action" -> {
            when (args.optString("name")) {
                "open_usage_settings" -> startActivity(Intent(Settings.ACTION_USAGE_ACCESS_SETTINGS).apply {
                    data = Uri.parse("package:$packageName")
                })
                "open_settings" -> startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$packageName")))
                "usage_read", "upload_now" -> Usage.read(applicationContext)
                else -> throw RuntimeException(tr("手机没有这个操作"))
            }
            JSONObject.NULL
        }
        else -> throw RuntimeException(tr("手机没有这个功能：{cmd}", mapOf("cmd" to cmd)))
    }

    private fun loggedIn(): Boolean = try {
        (Core.ok("sync_status") as? JSONObject)?.optBoolean("configured") ?: false
    } catch (_: Exception) { false }
}
