package com.lowkey.vpn.vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import com.lowkey.vpn.MainActivity
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import java.io.FileInputStream
import java.io.FileOutputStream
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.nio.ByteBuffer

class LowkeyVpnService : VpnService() {

    companion object {
        const val CHANNEL_ID = "lowkey_vpn_channel"
        const val NOTIFICATION_ID = 1001
    }

    private var vpnInterface: ParcelFileDescriptor? = null
    private var tunnelJob: Job? = null
    private val scope = CoroutineScope(Dispatchers.IO)

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val host = intent?.getStringExtra("host") ?: return START_NOT_STICKY
        val port = intent.getIntExtra("port", 8443)
        val token = intent.getStringExtra("token") ?: return START_NOT_STICKY
        val vpnIp = intent.getStringExtra("vpn_ip") ?: "10.0.0.2"

        createNotificationChannel()
        startForeground(NOTIFICATION_ID, buildNotification())
        startTunnel(host, port, token, vpnIp)
        return START_STICKY
    }

    private fun startTunnel(host: String, port: Int, token: String, vpnIp: String) {
        val builder = Builder()
            .setSession("Lowkey VPN")
            .addAddress(vpnIp, 24)
            .addRoute("0.0.0.0", 0)
            .addDnsServer("8.8.8.8")
            .addDnsServer("8.8.4.4")
            .setMtu(1400)

        vpnInterface = builder.establish() ?: return

        tunnelJob = scope.launch {
            runTunnel(host, port, token)
        }
    }

    private fun runTunnel(host: String, port: Int, token: String) {
        val tun = vpnInterface ?: return
        val inStream = FileInputStream(tun.fileDescriptor)
        val outStream = FileOutputStream(tun.fileDescriptor)

        try {
            val socket = DatagramSocket()
            protect(socket)

            val serverAddress = InetAddress.getByName(host)
            val buffer = ByteBuffer.allocate(32767)
            val packet = ByteArray(32767)

            // Send auth packet
            val authBytes = "AUTH:$token".toByteArray()
            socket.send(DatagramPacket(authBytes, authBytes.size, serverAddress, port))

            // Relay packets between VPN interface and UDP tunnel
            val readBuffer = ByteArray(32767)
            while (!Thread.interrupted()) {
                // Read from TUN, send to server
                val len = inStream.read(readBuffer)
                if (len > 0) {
                    socket.send(DatagramPacket(readBuffer, len, serverAddress, port))
                }

                // Read from server, write to TUN
                socket.soTimeout = 10
                try {
                    val incoming = DatagramPacket(packet, packet.size)
                    socket.receive(incoming)
                    outStream.write(packet, 0, incoming.length)
                } catch (e: Exception) {
                    // Timeout, continue
                }
            }
            socket.close()
        } catch (e: Exception) {
            // Connection lost
        }
    }

    override fun onDestroy() {
        tunnelJob?.cancel()
        vpnInterface?.close()
        vpnInterface = null
        super.onDestroy()
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "Lowkey VPN",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "VPN connection status"
            }
            getSystemService(NotificationManager::class.java)
                .createNotificationChannel(channel)
        }
    }

    private fun buildNotification(): Notification {
        val intent = Intent(this, MainActivity::class.java)
        val pendingIntent = PendingIntent.getActivity(
            this, 0, intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )

        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, CHANNEL_ID)
                .setContentTitle("Lowkey VPN")
                .setContentText("Подключено — трафик защищён")
                .setSmallIcon(android.R.drawable.ic_dialog_info)
                .setContentIntent(pendingIntent)
                .setOngoing(true)
                .build()
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
                .setContentTitle("Lowkey VPN")
                .setContentText("Подключено — трафик защищён")
                .setSmallIcon(android.R.drawable.ic_dialog_info)
                .setContentIntent(pendingIntent)
                .setOngoing(true)
                .build()
        }
    }
}
