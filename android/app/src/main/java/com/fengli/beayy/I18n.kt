package com.fengli.beayy

import android.content.Context
import android.util.Log
import org.json.JSONObject
import java.util.Locale

/**
 * 外壳自己的文字（报错、提示）的英文：和网页同一张表 ui/locales/en.js（build.sh 拷进 assets/ui/locales/en.js），
 * 以中文原句做键，写法同 app/ui/i18n.js 的 tr()。系统语言 zh 开头用中文、不查表；否则查表，没有就退回中文原句。
 * 网页设置里手选的语言存在 WebView 的 localStorage，这里读不到，只跟系统。
 */
object I18n {
    @Volatile private var app: Context? = null

    /** 进程入口（界面、后台任务）先调一次，tr 才找得到 assets */
    fun init(ctx: Context) { if (app == null) app = ctx.applicationContext }

    // 第一次要英文时才读；读不到（没 init、包里没有表）当空表
    private val dict: JSONObject by lazy {
        try {
            val s = app!!.assets.open("ui/locales/en.js").bufferedReader().use { it.readText() }
            val head = s.indexOf("window.I18N_EN")
            JSONObject(s.substring(s.indexOf('{', head), s.lastIndexOf('}') + 1))
        } catch (x: Exception) {
            Log.w("Beayy", "读英文译文出错", x)
            JSONObject()
        }
    }

    fun tr(s: String, vars: Map<String, String> = emptyMap()): String {
        var r: String = s
        if (Locale.getDefault().language != "zh") when (val v = dict.opt(s)) {
            is String -> r = v
            // 分单复数：按 n 选，和 Intl.PluralRules("en") 一样只有恰好 1 算 one
            is JSONObject -> r = v.optString(if (vars["n"]?.toDoubleOrNull() == 1.0) "one" else "other", null)
                ?: v.optString("other", s)
        }
        return if (vars.isEmpty()) r else Regex("""\{(\w+)\}""").replace(r) { m -> vars[m.groupValues[1]] ?: m.value }
    }
}

fun tr(s: String, vars: Map<String, String> = emptyMap()): String = I18n.tr(s, vars)
