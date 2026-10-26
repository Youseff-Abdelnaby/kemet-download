package com.kemet.download

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancelChildren
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

class DownloadService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private lateinit var nm: NotificationManager

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        nm = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            nm.createNotificationChannel(
                NotificationChannel(CH, "التحميلات", NotificationManager.IMPORTANCE_LOW)
            )
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        ServiceCompat.startForeground(
            this, NOTIF_ID, build("Kemet Download", "جاري التحضير...", 0, true, true),
            ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC
        )
        scope.coroutineContext.cancelChildren()
        scope.launch {
            while (isActive) {
                val list = DownloadManager.items.value
                val active = list.filter { it.status == Status.DOWNLOADING }
                val waiting = list.count { it.status == Status.WAITING }
                if (active.isEmpty() && waiting == 0) break
                val avg = if (active.isNotEmpty()) active.map { it.pct }.average().toInt() else 0
                val text = "${active.firstOrNull()?.line.orEmpty()} • بالانتظار: $waiting"
                nm.notify(NOTIF_ID, build("جاري تحميل ${active.size} ملف — $avg%", text, avg, false, true))
                delay(1000)
            }
            val list = DownloadManager.items.value
            val ok = list.count { it.status == Status.DONE }
            val bad = list.count { it.status == Status.ERROR }
            ServiceCompat.stopForeground(this@DownloadService, ServiceCompat.STOP_FOREGROUND_REMOVE)
            if (ok + bad > 0) {
                nm.notify(NOTIF_ID + 1, build("انتهى التحميل 🏆", "اكتمل: $ok — فشل: $bad", 0, false, false))
            }
            stopSelf()
        }
        return START_NOT_STICKY
    }

    private fun build(title: String, text: String, pct: Int, indeterminate: Boolean, ongoing: Boolean): Notification {
        val pi = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val b = NotificationCompat.Builder(this, CH)
            .setSmallIcon(R.drawable.ic_stat_download)
            .setContentTitle(title)
            .setContentText(text)
            .setContentIntent(pi)
            .setOnlyAlertOnce(true)
            .setOngoing(ongoing)
            .setAutoCancel(!ongoing)
        if (ongoing) b.setProgress(100, pct, indeterminate)
        return b.build()
    }

    override fun onDestroy() {
        scope.coroutineContext.cancelChildren()
        super.onDestroy()
    }

    companion object {
        const val CH = "kemet_dl"
        const val NOTIF_ID = 1001
    }
}
