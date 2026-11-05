package com.fengli.beayy

import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.ComponentName
import android.content.Context

/** 每 15 分钟（系统允许的最短间隔）在后台读一轮使用记录并上传；有网才跑，重启后接着排 */
class UsageJob : JobService() {
    override fun onStartJob(params: JobParameters): Boolean {
        I18n.init(this)
        Thread {
            Usage.read(applicationContext)
            jobFinished(params, false)
        }.start()
        return true
    }

    override fun onStopJob(params: JobParameters): Boolean = true

    companion object {
        private const val ID = 1

        fun schedule(ctx: Context) {
            val js = ctx.getSystemService(JobScheduler::class.java)
            if (js.getPendingJob(ID) != null) return
            js.schedule(
                JobInfo.Builder(ID, ComponentName(ctx, UsageJob::class.java))
                    .setPeriodic(15 * 60 * 1000L)
                    .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                    .setPersisted(true)
                    .build()
            )
        }
    }
}
