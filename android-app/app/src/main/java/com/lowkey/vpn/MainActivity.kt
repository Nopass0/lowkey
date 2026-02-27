package com.lowkey.vpn

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import com.lowkey.vpn.ui.LowkeyAppWithModal
import com.lowkey.vpn.ui.theme.LowkeyTheme
import com.lowkey.vpn.workers.scheduleExpiryCheck

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        // Schedule background subscription expiry notification check
        scheduleExpiryCheck(this)

        setContent {
            LowkeyTheme {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = MaterialTheme.colorScheme.background
                ) {
                    LowkeyAppWithModal()
                }
            }
        }
    }
}
