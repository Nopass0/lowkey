package com.lowkey.vpn.workers

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import com.lowkey.vpn.MainActivity
import com.lowkey.vpn.data.LowkeyApiService
import java.util.concurrent.TimeUnit

class ExpiryCheckWorker(
    context: Context,
    params: WorkerParameters
) : CoroutineWorker(context, params) {

    companion object {
        const val WORK_NAME = "lowkey_expiry_check"
        const val CHANNEL_ID = "lowkey_expiry_channel"
        const val NOTIFICATION_ID = 2001
    }

    override suspend fun doWork(): Result {
        val api = LowkeyApiService(applicationContext)
        if (api.token == null) return Result.success()

        api.me().onSuccess { user ->
            val expiresAt = user.subExpiresAt ?: return@onSuccess
            val now = System.currentTimeMillis()
            val daysLeft = ((expiresAt - now) / 86400000L).toInt()

            if (daysLeft in 1..3) {
                sendExpiryNotification(daysLeft)
            } else if (daysLeft <= 0 && user.subStatus == "active") {
                sendExpiredNotification()
            }
        }

        return Result.success()
    }

    private fun sendExpiryNotification(daysLeft: Int) {
        createChannel()
        val intent = Intent(applicationContext, MainActivity::class.java)
        val pending = PendingIntent.getActivity(
            applicationContext, 0, intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )

        val notification = NotificationCompat.Builder(applicationContext, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_dialog_alert)
            .setContentTitle("Lowkey VPN — подписка истекает")
            .setContentText("Осталось $daysLeft дн. Продлите подписку.")
            .setContentIntent(pending)
            .setAutoCancel(true)
            .build()

        applicationContext.getSystemService(NotificationManager::class.java)
            .notify(NOTIFICATION_ID, notification)
    }

    private fun sendExpiredNotification() {
        createChannel()
        val intent = Intent(applicationContext, MainActivity::class.java)
        val pending = PendingIntent.getActivity(
            applicationContext, 0, intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )

        val notification = NotificationCompat.Builder(applicationContext, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_dialog_alert)
            .setContentTitle("Lowkey VPN — подписка истекла")
            .setContentText("Ваша подписка закончилась. Нажмите для продления.")
            .setContentIntent(pending)
            .setAutoCancel(true)
            .build()

        applicationContext.getSystemService(NotificationManager::class.java)
            .notify(NOTIFICATION_ID + 1, notification)
    }

    private fun createChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "Уведомления о подписке",
                NotificationManager.IMPORTANCE_DEFAULT
            )
            applicationContext.getSystemService(NotificationManager::class.java)
                .createNotificationChannel(channel)
        }
    }
}

fun scheduleExpiryCheck(context: Context) {
    val request = PeriodicWorkRequestBuilder<ExpiryCheckWorker>(
        12, TimeUnit.HOURS
    ).build()

    WorkManager.getInstance(context).enqueueUniquePeriodicWork(
        ExpiryCheckWorker.WORK_NAME,
        ExistingPeriodicWorkPolicy.KEEP,
        request
    )
}
