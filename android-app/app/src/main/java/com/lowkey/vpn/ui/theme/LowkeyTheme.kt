package com.lowkey.vpn.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

private val LowkeyColorScheme = darkColorScheme(
    primary = Color(0xFF00FF88),
    onPrimary = Color.Black,
    primaryContainer = Color(0xFF003319),
    secondary = Color(0xFF0066FF),
    onSecondary = Color.White,
    background = Color(0xFF06060F),
    onBackground = Color(0xFFF0F4FF),
    surface = Color(0xFF0D0D1F),
    onSurface = Color(0xFFF0F4FF),
    error = Color(0xFFFF4444),
    onError = Color.White,
)

@Composable
fun LowkeyTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = LowkeyColorScheme,
        content = content
    )
}
