package com.lowkey.vpn.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.widget.Toast
import androidx.compose.animation.*
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.lowkey.vpn.ui.viewmodel.MainViewModel
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

// ── Brand colours ─────────────────────────────────────────────────────────────

val BgColor     = Color(0xFF06060F)
val CardColor   = Color(0xFF0D0D1F)
val GreenColor  = Color(0xFF00FF88)
val BlueColor   = Color(0xFF0066FF)
val TextColor   = Color(0xFFF0F4FF)
val MutedColor  = Color(0xFF8892B0)
val DangerColor = Color(0xFFFF4444)
val WarnColor   = Color(0xFFFFAA00)

// ── Root ──────────────────────────────────────────────────────────────────────

@Composable
fun LowkeyApp(vm: MainViewModel = viewModel()) {
    val token by vm.token.collectAsState()

    Box(Modifier.fillMaxSize().background(BgColor)) {
        AnimatedContent(targetState = token != null, label = "auth") { isLoggedIn ->
            if (isLoggedIn) MainScreen(vm = vm) else AuthScreen(vm = vm)
        }
    }
}

// ── Auth screen (login + register tabs) ───────────────────────────────────────

@Composable
fun AuthScreen(vm: MainViewModel) {
    var tab by remember { mutableIntStateOf(0) }        // 0 = login, 1 = register
    val isLoading by vm.isLoading.collectAsState()
    val error     by vm.error.collectAsState()

    // Login fields
    var loginField    by remember { mutableStateOf("") }
    var passwordField by remember { mutableStateOf("") }
    var showPw        by remember { mutableStateOf(false) }

    // Register fields
    var regLogin    by remember { mutableStateOf("") }
    var regPassword by remember { mutableStateOf("") }
    var regConfirm  by remember { mutableStateOf("") }
    var regRef      by remember { mutableStateOf("") }
    var regShowPw   by remember { mutableStateOf(false) }

    LaunchedEffect(tab) { vm.clearError() }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        // Logo
        Box(
            Modifier.size(72.dp).background(
                Brush.linearGradient(listOf(GreenColor, BlueColor)),
                RoundedCornerShape(18.dp)
            ),
            contentAlignment = Alignment.Center
        ) {
            Icon(Icons.Default.Lock, null, tint = Color.Black, modifier = Modifier.size(36.dp))
        }
        Spacer(Modifier.height(16.dp))
        Text("Lowkey VPN", fontSize = 26.sp, fontWeight = FontWeight.Bold, color = GreenColor)
        Text("Безопасный и быстрый VPN", fontSize = 13.sp, color = MutedColor)
        Spacer(Modifier.height(28.dp))

        // Tabs
        Row(
            Modifier.fillMaxWidth().background(CardColor, RoundedCornerShape(12.dp)).padding(4.dp)
        ) {
            listOf("Войти", "Регистрация").forEachIndexed { i, label ->
                Box(
                    Modifier.weight(1f)
                        .background(
                            if (tab == i) GreenColor.copy(alpha = 0.15f) else Color.Transparent,
                            RoundedCornerShape(8.dp)
                        )
                        .clickable { tab = i }
                        .padding(vertical = 10.dp),
                    contentAlignment = Alignment.Center
                ) {
                    Text(
                        label,
                        color = if (tab == i) GreenColor else MutedColor,
                        fontWeight = if (tab == i) FontWeight.Bold else FontWeight.Normal,
                        fontSize = 14.sp
                    )
                }
            }
        }

        Spacer(Modifier.height(16.dp))

        error?.let { msg ->
            Card(
                Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(containerColor = DangerColor.copy(alpha = 0.1f)),
                shape = RoundedCornerShape(12.dp)
            ) {
                Text(msg, Modifier.padding(12.dp), color = DangerColor, fontSize = 13.sp)
            }
            Spacer(Modifier.height(12.dp))
        }

        if (tab == 0) {
            // ── Login form ────────────────────────────────────────────────────
            LowkeyTextField(value = loginField, onValueChange = { loginField = it },
                label = "Логин", placeholder = "Введите логин")
            Spacer(Modifier.height(12.dp))
            LowkeyTextField(value = passwordField, onValueChange = { passwordField = it },
                label = "Пароль", placeholder = "Введите пароль",
                isPassword = true, showPassword = showPw,
                onTogglePassword = { showPw = !showPw })
            Spacer(Modifier.height(20.dp))
            Button(
                onClick = { vm.login(loginField.trim(), passwordField) },
                enabled = !isLoading && loginField.isNotBlank() && passwordField.isNotBlank(),
                modifier = Modifier.fillMaxWidth().height(52.dp),
                colors = ButtonDefaults.buttonColors(containerColor = GreenColor),
                shape = RoundedCornerShape(12.dp)
            ) {
                if (isLoading) CircularProgressIndicator(Modifier.size(20.dp), color = Color.Black, strokeWidth = 2.dp)
                else Text("Войти", color = Color.Black, fontWeight = FontWeight.Bold)
            }
        } else {
            // ── Register form ─────────────────────────────────────────────────
            LowkeyTextField(value = regLogin, onValueChange = { regLogin = it },
                label = "Логин", placeholder = "3–50 символов")
            Spacer(Modifier.height(10.dp))
            LowkeyTextField(value = regPassword, onValueChange = { regPassword = it },
                label = "Пароль", placeholder = "Минимум 6 символов",
                isPassword = true, showPassword = regShowPw,
                onTogglePassword = { regShowPw = !regShowPw })
            Spacer(Modifier.height(10.dp))
            LowkeyTextField(value = regConfirm, onValueChange = { regConfirm = it },
                label = "Подтвердите пароль", placeholder = "Повторите пароль",
                isPassword = true, showPassword = regShowPw)
            Spacer(Modifier.height(10.dp))
            LowkeyTextField(value = regRef, onValueChange = { regRef = it.uppercase() },
                label = "Реферальный код (необязательно)", placeholder = "XXXXXXXX")
            Spacer(Modifier.height(20.dp))
            Button(
                onClick = {
                    if (regPassword != regConfirm) {
                        vm.clearError()
                        return@Button
                    }
                    vm.register(regLogin.trim(), regPassword, regRef.ifBlank { null })
                },
                enabled = !isLoading && regLogin.length >= 3 && regPassword.length >= 6 && regPassword == regConfirm,
                modifier = Modifier.fillMaxWidth().height(52.dp),
                colors = ButtonDefaults.buttonColors(containerColor = GreenColor),
                shape = RoundedCornerShape(12.dp)
            ) {
                if (isLoading) CircularProgressIndicator(Modifier.size(20.dp), color = Color.Black, strokeWidth = 2.dp)
                else Text("Создать аккаунт", color = Color.Black, fontWeight = FontWeight.Bold)
            }
            if (regPassword.isNotEmpty() && regConfirm.isNotEmpty() && regPassword != regConfirm) {
                Spacer(Modifier.height(8.dp))
                Text("Пароли не совпадают", color = DangerColor, fontSize = 12.sp)
            }
        }
    }
}

// ── Main screen ───────────────────────────────────────────────────────────────

@Composable
fun MainScreen(vm: MainViewModel) {
    val user      by vm.user.collectAsState()
    val connected by vm.connected.collectAsState()
    val toggling  by vm.toggling.collectAsState()
    val success   by vm.successMsg.collectAsState()
    val context   = LocalContext.current

    var selectedTab by remember { mutableIntStateOf(0) }
    val tabs = listOf("Главная", "Тарифы", "Рефералы", "История")

    // Show success messages as Toasts
    LaunchedEffect(success) {
        if (success != null) {
            Toast.makeText(context, success, Toast.LENGTH_LONG).show()
            vm.clearSuccess()
        }
    }

    Column(Modifier.fillMaxSize()) {
        // Header
        Row(
            Modifier.fillMaxWidth().background(CardColor)
                .padding(horizontal = 16.dp, vertical = 12.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text("Lowkey VPN", fontSize = 18.sp, fontWeight = FontWeight.Bold, color = GreenColor)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(user?.login ?: "", fontSize = 13.sp, color = MutedColor)
                Spacer(Modifier.width(4.dp))
                IconButton(onClick = { vm.refreshAll() }) {
                    Icon(Icons.Default.Refresh, "Обновить", tint = MutedColor)
                }
                IconButton(onClick = { vm.logout() }) {
                    Icon(Icons.Default.ExitToApp, "Выйти", tint = MutedColor)
                }
            }
        }

        // Tab row
        TabRow(selectedTabIndex = selectedTab, containerColor = CardColor, contentColor = GreenColor) {
            tabs.forEachIndexed { index, tab ->
                Tab(
                    selected = selectedTab == index,
                    onClick = { selectedTab = index },
                    text = { Text(tab, fontSize = 11.sp) }
                )
            }
        }

        Box(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)) {
            when (selectedTab) {
                0 -> HomeTab(vm, user, connected, toggling)
                1 -> PlansTab(vm)
                2 -> ReferralTab(vm)
                3 -> HistoryTab(vm)
            }
        }
    }

    PaymentModal(vm)
}

// ── Home tab ──────────────────────────────────────────────────────────────────

@Composable
fun HomeTab(vm: MainViewModel, user: com.lowkey.vpn.data.UserModel?, connected: Boolean, toggling: Boolean) {
    val error by vm.error.collectAsState()

    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Spacer(Modifier.height(16.dp))

        error?.let { msg ->
            Card(
                Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(containerColor = DangerColor.copy(alpha = 0.1f)),
                shape = RoundedCornerShape(12.dp)
            ) {
                Row(Modifier.padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Default.Warning, null, tint = DangerColor)
                    Spacer(Modifier.width(8.dp))
                    Text(msg, color = DangerColor, fontSize = 13.sp)
                }
            }
            Spacer(Modifier.height(12.dp))
        }

        // VPN toggle button
        val btnColor = if (connected) GreenColor else DangerColor
        Button(
            onClick = { if (!toggling) vm.toggleVpn() },
            modifier = Modifier.size(150.dp),
            shape = CircleShape,
            colors = ButtonDefaults.buttonColors(containerColor = btnColor.copy(alpha = 0.12f)),
            border = androidx.compose.foundation.BorderStroke(3.dp, btnColor)
        ) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                if (toggling) {
                    CircularProgressIndicator(Modifier.size(32.dp), color = btnColor, strokeWidth = 3.dp)
                } else {
                    Icon(
                        if (connected) Icons.Default.Wifi else Icons.Default.WifiOff,
                        null, tint = btnColor, modifier = Modifier.size(42.dp)
                    )
                }
            }
        }

        Spacer(Modifier.height(10.dp))
        Text(
            if (toggling) "Подключение..." else if (connected) "Подключён" else "Отключён",
            fontSize = 18.sp, fontWeight = FontWeight.Bold,
            color = if (connected) GreenColor else DangerColor
        )
        Text(
            if (connected) "Трафик защищён" else "Нажмите для подключения",
            fontSize = 13.sp, color = MutedColor
        )

        Spacer(Modifier.height(24.dp))

        // Stats row
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            InfoCard(Modifier.weight(1f), "Баланс", "${user?.balance?.toInt() ?: 0} ₽") {
                vm.openPayModal()
            }
            InfoCard(
                modifier = Modifier.weight(1f),
                title = "Подписка",
                value = when (user?.subStatus) {
                    "active" -> "Активна"
                    "expired" -> "Истекла"
                    else -> "Неактивна"
                },
                valueColor = if (user?.subStatus == "active") GreenColor else DangerColor
            )
        }

        // Expiry warning
        user?.subExpiresAt?.let { exp ->
            val daysLeft = daysLeft(exp)
            if (daysLeft in 1..5) {
                Spacer(Modifier.height(12.dp))
                Card(
                    Modifier.fillMaxWidth(),
                    colors = CardDefaults.cardColors(containerColor = WarnColor.copy(alpha = 0.1f)),
                    shape = RoundedCornerShape(12.dp)
                ) {
                    Row(Modifier.padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Default.Warning, null, tint = WarnColor)
                        Spacer(Modifier.width(8.dp))
                        Text(
                            "Подписка истекает через $daysLeft ${dayLabel(daysLeft)}",
                            color = WarnColor, fontSize = 13.sp
                        )
                    }
                }
            }
        }

        // Subscription info (speed + expiry)
        if (user?.subStatus == "active") {
            Spacer(Modifier.height(12.dp))
            Card(
                Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(containerColor = CardColor),
                shape = RoundedCornerShape(12.dp)
            ) {
                Column(Modifier.padding(16.dp)) {
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                        Text("Скорость", fontSize = 13.sp, color = MutedColor)
                        Text(
                            if (user?.subSpeedMbps == 0.0) "Без ограничений" else "${user?.subSpeedMbps?.toInt()} Мбит/с",
                            fontSize = 13.sp, fontWeight = FontWeight.Bold, color = TextColor
                        )
                    }
                    user?.subExpiresAt?.let { exp ->
                        Spacer(Modifier.height(8.dp))
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                            Text("Действует до", fontSize = 13.sp, color = MutedColor)
                            Text(formatDate(exp), fontSize = 13.sp, fontWeight = FontWeight.Bold, color = TextColor)
                        }
                        Spacer(Modifier.height(8.dp))
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                            Text("Осталось", fontSize = 13.sp, color = MutedColor)
                            Text("${daysLeft(exp)} дн.", fontSize = 13.sp, fontWeight = FontWeight.Bold, color = GreenColor)
                        }
                    }
                }
            }
        }

        // Promo code section
        Spacer(Modifier.height(12.dp))
        PromoSection(vm)
    }
}

@Composable
fun PromoSection(vm: MainViewModel) {
    var code     by remember { mutableStateOf("") }
    val loading  by vm.isLoading.collectAsState()

    Card(
        Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = CardColor),
        shape = RoundedCornerShape(12.dp)
    ) {
        Column(Modifier.padding(16.dp)) {
            Text("Промокод", fontSize = 14.sp, fontWeight = FontWeight.Bold, color = TextColor)
            Spacer(Modifier.height(10.dp))
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = code,
                    onValueChange = { code = it.uppercase() },
                    modifier = Modifier.weight(1f),
                    placeholder = { Text("XXXXXXXX", color = MutedColor) },
                    singleLine = true,
                    colors = OutlinedTextFieldDefaults.colors(
                        focusedBorderColor = GreenColor,
                        unfocusedBorderColor = Color(0xFF1A1A3E),
                        focusedTextColor = TextColor, unfocusedTextColor = TextColor,
                        cursorColor = GreenColor
                    ),
                    shape = RoundedCornerShape(10.dp)
                )
                Button(
                    onClick = { vm.applyPromo(code.trim()); code = "" },
                    enabled = !loading && code.length >= 4,
                    colors = ButtonDefaults.buttonColors(containerColor = GreenColor),
                    shape = RoundedCornerShape(10.dp),
                    modifier = Modifier.height(56.dp)
                ) {
                    Text("OK", color = Color.Black, fontWeight = FontWeight.Bold)
                }
            }
        }
    }
}

// ── Plans tab ─────────────────────────────────────────────────────────────────

@Composable
fun PlansTab(vm: MainViewModel) {
    val plans by vm.plans.collectAsState()

    Column {
        Text("Тарифы", fontSize = 20.sp, fontWeight = FontWeight.Bold, color = TextColor)
        Text("Оплата через СБП. Скидка 50% при первой покупке по реферальной ссылке.",
            fontSize = 12.sp, color = MutedColor, modifier = Modifier.padding(top = 4.dp, bottom = 16.dp))

        plans.forEach { plan ->
            Card(
                Modifier.fillMaxWidth().padding(bottom = 12.dp),
                colors = CardDefaults.cardColors(containerColor = CardColor),
                shape = RoundedCornerShape(16.dp)
            ) {
                Column(Modifier.padding(16.dp)) {
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.Top) {
                        Column {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Text(plan.name, fontWeight = FontWeight.Bold, color = TextColor, fontSize = 16.sp)
                                if (plan.isBundle && plan.discountPct > 0) {
                                    Spacer(Modifier.width(8.dp))
                                    Badge(containerColor = GreenColor.copy(alpha = 0.2f)) {
                                        Text("−${plan.discountPct}%", color = GreenColor, fontSize = 10.sp)
                                    }
                                }
                            }
                            Text(
                                "${if (plan.speedMbps == 0.0) "∞" else plan.speedMbps.toInt()} Мбит/с · ${plan.durationDays} дней",
                                fontSize = 13.sp, color = MutedColor, modifier = Modifier.padding(top = 2.dp)
                            )
                        }
                        Column(horizontalAlignment = Alignment.End) {
                            Text("${plan.priceRub.toInt()} ₽", fontWeight = FontWeight.Bold,
                                color = GreenColor, fontSize = 20.sp)
                        }
                    }
                    Spacer(Modifier.height(12.dp))
                    Button(
                        onClick = { vm.createPayment(plan.priceRub, "subscription", plan.planKey) },
                        modifier = Modifier.fillMaxWidth(),
                        colors = ButtonDefaults.outlinedButtonColors(contentColor = GreenColor),
                        border = androidx.compose.foundation.BorderStroke(1.dp, GreenColor.copy(alpha = 0.5f)),
                        shape = RoundedCornerShape(10.dp)
                    ) {
                        Icon(Icons.Default.QrCode, null, Modifier.size(16.dp), tint = GreenColor)
                        Spacer(Modifier.width(6.dp))
                        Text("Оплатить через СБП", color = GreenColor)
                    }
                }
            }
        }

        if (plans.isEmpty()) {
            Box(Modifier.fillMaxWidth().padding(32.dp), contentAlignment = Alignment.Center) {
                CircularProgressIndicator(color = GreenColor)
            }
        }
    }
}

// ── Referral tab ──────────────────────────────────────────────────────────────

@Composable
fun ReferralTab(vm: MainViewModel) {
    val refStats    by vm.refStats.collectAsState()
    val withdrawals by vm.withdrawals.collectAsState()
    val loading     by vm.isLoading.collectAsState()
    val context     = LocalContext.current

    var showWithdrawDialog by remember { mutableStateOf(false) }
    var withdrawAmount     by remember { mutableStateOf("") }
    var withdrawCard       by remember { mutableStateOf("") }
    var withdrawBank       by remember { mutableStateOf("") }

    Column {
        Text("Реферальная программа", fontSize = 20.sp, fontWeight = FontWeight.Bold, color = TextColor)
        Spacer(Modifier.height(4.dp))
        Text("Получайте 25% с каждого платежа ваших друзей", fontSize = 12.sp, color = MutedColor)
        Spacer(Modifier.height(16.dp))

        refStats?.let { stats ->
            // Balance card
            Card(Modifier.fillMaxWidth(), colors = CardDefaults.cardColors(containerColor = CardColor),
                shape = RoundedCornerShape(16.dp)) {
                Column(Modifier.padding(16.dp)) {
                    Text("Реферальный баланс", fontSize = 12.sp, color = MutedColor)
                    Text("${stats.referralBalance.toInt()} ₽", fontSize = 32.sp,
                        fontWeight = FontWeight.Bold, color = GreenColor)
                    Spacer(Modifier.height(12.dp))
                    Button(
                        onClick = { showWithdrawDialog = true },
                        enabled = stats.referralBalance >= 100,
                        modifier = Modifier.fillMaxWidth(),
                        colors = ButtonDefaults.buttonColors(containerColor = GreenColor),
                        shape = RoundedCornerShape(10.dp)
                    ) {
                        Text("Вывести на карту (мин. 100 ₽)", color = Color.Black, fontWeight = FontWeight.Bold)
                    }
                    if (stats.referralBalance < 100) {
                        Text("Минимальная сумма вывода: 100 ₽", fontSize = 11.sp, color = MutedColor,
                            modifier = Modifier.padding(top = 4.dp))
                    }
                }
            }

            Spacer(Modifier.height(12.dp))

            // Stats row
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                StatMini(Modifier.weight(1f), "${stats.referralCount}", "Приглашено")
                StatMini(Modifier.weight(1f), "${stats.totalEarned.toInt()} ₽", "Заработано")
            }

            Spacer(Modifier.height(12.dp))

            // Referral code
            stats.referralCode?.let { code ->
                Card(Modifier.fillMaxWidth(), colors = CardDefaults.cardColors(containerColor = CardColor),
                    shape = RoundedCornerShape(16.dp)) {
                    Column(Modifier.padding(16.dp)) {
                        Text("Ваш реферальный код", fontSize = 12.sp, color = MutedColor)
                        Spacer(Modifier.height(8.dp))
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically) {
                            Text(code, fontSize = 24.sp, fontWeight = FontWeight.Bold,
                                color = GreenColor, fontFamily = FontFamily.Monospace)
                            IconButton(onClick = {
                                val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                                clipboard.setPrimaryClip(ClipData.newPlainText("ref_code", code))
                                Toast.makeText(context, "Код скопирован!", Toast.LENGTH_SHORT).show()
                            }) {
                                Icon(Icons.Default.ContentCopy, "Копировать", tint = GreenColor)
                            }
                        }
                        Text("Ваш друг получит скидку 50% на первую подписку",
                            fontSize = 12.sp, color = MutedColor, modifier = Modifier.padding(top = 4.dp))
                    }
                }
            }

            // Withdrawal history
            if (withdrawals.isNotEmpty()) {
                Spacer(Modifier.height(16.dp))
                Text("История выводов", fontSize = 16.sp, fontWeight = FontWeight.Bold, color = TextColor)
                Spacer(Modifier.height(8.dp))
                withdrawals.take(5).forEach { w ->
                    WithdrawalRow(w)
                    Spacer(Modifier.height(6.dp))
                }
            }
        } ?: Box(Modifier.fillMaxWidth().padding(32.dp), contentAlignment = Alignment.Center) {
            CircularProgressIndicator(color = GreenColor)
        }
    }

    // Withdrawal dialog
    if (showWithdrawDialog) {
        AlertDialog(
            onDismissRequest = { showWithdrawDialog = false },
            containerColor = CardColor,
            title = { Text("Вывод средств", color = TextColor, fontWeight = FontWeight.Bold) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    LowkeyTextField(withdrawAmount, { withdrawAmount = it },
                        "Сумма (₽)", "Минимум 100 ₽", keyboardType = KeyboardType.Number)
                    LowkeyTextField(withdrawCard, { withdrawCard = it },
                        "Номер карты", "1234 5678 9012 3456", keyboardType = KeyboardType.Number)
                    LowkeyTextField(withdrawBank, { withdrawBank = it },
                        "Банк (необязательно)", "Сбербанк / Тинькофф / ВТБ")
                }
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        val amt = withdrawAmount.toDoubleOrNull() ?: 0.0
                        if (amt >= 100 && withdrawCard.length >= 16) {
                            vm.requestWithdrawal(amt, withdrawCard.filter { it.isDigit() }, withdrawBank.ifBlank { null })
                            showWithdrawDialog = false
                            withdrawAmount = ""; withdrawCard = ""; withdrawBank = ""
                        }
                    },
                    enabled = !loading && (withdrawAmount.toDoubleOrNull() ?: 0.0) >= 100 && withdrawCard.length >= 16
                ) {
                    Text("Вывести", color = GreenColor, fontWeight = FontWeight.Bold)
                }
            },
            dismissButton = {
                TextButton(onClick = { showWithdrawDialog = false }) {
                    Text("Отмена", color = MutedColor)
                }
            }
        )
    }
}

@Composable
fun WithdrawalRow(w: com.lowkey.vpn.data.WithdrawalModel) {
    val statusColor = when (w.status) {
        "completed"  -> GreenColor
        "rejected"   -> DangerColor
        "processing" -> WarnColor
        else         -> MutedColor
    }
    val statusLabel = when (w.status) {
        "completed"  -> "Выплачено"
        "rejected"   -> "Отклонено"
        "processing" -> "Обработка"
        "pending"    -> "Ожидание"
        else         -> w.status
    }
    Card(Modifier.fillMaxWidth(), colors = CardDefaults.cardColors(containerColor = CardColor),
        shape = RoundedCornerShape(10.dp)) {
        Row(Modifier.padding(12.dp).fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically) {
            Column {
                Text("${w.amount.toInt()} ₽", fontWeight = FontWeight.Bold, color = TextColor)
                Text(w.cardNumber.takeLast(4).let { "•••• $it" }, fontSize = 12.sp, color = MutedColor)
            }
            Column(horizontalAlignment = Alignment.End) {
                Text(statusLabel, color = statusColor, fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
                w.adminNote?.let { note ->
                    Text(note, fontSize = 11.sp, color = MutedColor, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}

// ── History tab ───────────────────────────────────────────────────────────────

@Composable
fun HistoryTab(vm: MainViewModel) {
    val history by vm.payHistory.collectAsState()

    Column {
        Text("История платежей", fontSize = 20.sp, fontWeight = FontWeight.Bold, color = TextColor)
        Spacer(Modifier.height(16.dp))

        if (history.isEmpty()) {
            Box(Modifier.fillMaxWidth().padding(vertical = 40.dp), contentAlignment = Alignment.Center) {
                Text("Нет платежей", color = MutedColor, fontSize = 14.sp)
            }
        } else {
            history.forEach { item ->
                val statusColor = when (item.status) {
                    "paid"    -> GreenColor
                    "pending" -> WarnColor
                    else      -> DangerColor
                }
                val statusLabel = when (item.status) {
                    "paid"    -> "Оплачено"
                    "pending" -> "Ожидание"
                    "expired" -> "Истёк"
                    "failed"  -> "Ошибка"
                    else      -> item.status
                }
                Card(
                    Modifier.fillMaxWidth().padding(bottom = 8.dp),
                    colors = CardDefaults.cardColors(containerColor = CardColor),
                    shape = RoundedCornerShape(12.dp)
                ) {
                    Row(Modifier.padding(16.dp).fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically) {
                        Column {
                            Text(
                                when (item.purpose) {
                                    "subscription" -> "Подписка${item.planId?.let { " ($it)" } ?: ""}"
                                    "balance"      -> "Пополнение баланса"
                                    else           -> item.purpose
                                },
                                fontWeight = FontWeight.SemiBold, color = TextColor, fontSize = 14.sp
                            )
                            Text(
                                item.paidAt?.let { formatDate(it) } ?: formatDate(item.createdAt),
                                fontSize = 12.sp, color = MutedColor
                            )
                        }
                        Column(horizontalAlignment = Alignment.End) {
                            Text("${item.amount.toInt()} ₽", fontWeight = FontWeight.Bold,
                                color = TextColor, fontSize = 16.sp)
                            Text(statusLabel, color = statusColor, fontSize = 12.sp)
                        }
                    }
                }
            }
        }
    }
}

// ── Payment modal ─────────────────────────────────────────────────────────────

@Composable
fun PaymentModal(vm: MainViewModel) {
    val showPayModal  by vm.showPayModal.collectAsState()
    val qrUrl         by vm.paymentQrUrl.collectAsState()
    val paymentStatus by vm.paymentStatus.collectAsState()
    val isLoading     by vm.isLoading.collectAsState()
    val context       = LocalContext.current

    if (!showPayModal) return

    AlertDialog(
        onDismissRequest = { vm.closePayModal() },
        containerColor = CardColor,
        title = {
            Text(
                if (paymentStatus == "paid") "Оплата прошла!" else "Пополнение через СБП",
                color = if (paymentStatus == "paid") GreenColor else TextColor,
                fontWeight = FontWeight.Bold
            )
        },
        text = {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                when {
                    paymentStatus == "paid" -> {
                        Icon(Icons.Default.CheckCircle, null, tint = GreenColor, modifier = Modifier.size(64.dp))
                        Spacer(Modifier.height(8.dp))
                        Text("Баланс пополнен", color = TextColor)
                    }
                    isLoading -> {
                        CircularProgressIndicator(color = GreenColor)
                        Spacer(Modifier.height(8.dp))
                        Text("Создание платежа...", color = MutedColor, fontSize = 13.sp)
                    }
                    qrUrl != null -> {
                        Text("Нажмите кнопку ниже для оплаты через СБП в вашем банке",
                            color = MutedColor, fontSize = 13.sp, textAlign = TextAlign.Center)
                        Spacer(Modifier.height(12.dp))
                        Button(
                            onClick = {
                                val intent = Intent(Intent.ACTION_VIEW, Uri.parse(qrUrl))
                                context.startActivity(intent)
                            },
                            colors = ButtonDefaults.buttonColors(containerColor = GreenColor),
                            shape = RoundedCornerShape(10.dp), modifier = Modifier.fillMaxWidth()
                        ) {
                            Text("Оплатить в банке", color = Color.Black, fontWeight = FontWeight.Bold)
                        }
                        Spacer(Modifier.height(8.dp))
                        Text(
                            if (paymentStatus == "pending") "Ожидание платежа..." else "Статус: $paymentStatus",
                            color = MutedColor, fontSize = 12.sp
                        )
                        if (paymentStatus == "pending") {
                            Spacer(Modifier.height(4.dp))
                            LinearProgressIndicator(color = GreenColor,
                                trackColor = GreenColor.copy(alpha = 0.2f),
                                modifier = Modifier.fillMaxWidth())
                        }
                    }
                    else -> {
                        Text("Выберите сумму пополнения", color = MutedColor, fontSize = 13.sp)
                        Spacer(Modifier.height(12.dp))
                        listOf(100, 300, 500, 1000).forEach { amount ->
                            Button(
                                onClick = { vm.createPayment(amount.toDouble(), "balance") },
                                modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                                colors = ButtonDefaults.outlinedButtonColors(contentColor = GreenColor),
                                border = androidx.compose.foundation.BorderStroke(1.dp, GreenColor.copy(alpha = 0.5f)),
                                shape = RoundedCornerShape(10.dp)
                            ) {
                                Text("$amount ₽", color = GreenColor)
                            }
                        }
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { vm.closePayModal() }) {
                Text("Закрыть", color = MutedColor)
            }
        }
    )
}

// ── Reusable components ───────────────────────────────────────────────────────

@Composable
fun InfoCard(
    modifier: Modifier,
    title: String,
    value: String,
    valueColor: Color = TextColor,
    onClick: (() -> Unit)? = null
) {
    Card(modifier = modifier, colors = CardDefaults.cardColors(containerColor = CardColor),
        shape = RoundedCornerShape(16.dp),
        onClick = { onClick?.invoke() }) {
        Column(Modifier.padding(16.dp)) {
            Text(title, fontSize = 12.sp, color = MutedColor)
            Spacer(Modifier.height(4.dp))
            Text(value, fontSize = 18.sp, fontWeight = FontWeight.Bold, color = valueColor)
            if (onClick != null) {
                Spacer(Modifier.height(6.dp))
                Text("Пополнить СБП", fontSize = 11.sp, color = GreenColor)
            }
        }
    }
}

@Composable
fun StatMini(modifier: Modifier, value: String, label: String) {
    Card(modifier = modifier, colors = CardDefaults.cardColors(containerColor = CardColor),
        shape = RoundedCornerShape(12.dp)) {
        Column(Modifier.padding(12.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Text(value, fontSize = 22.sp, fontWeight = FontWeight.Bold, color = GreenColor)
            Text(label, fontSize = 12.sp, color = MutedColor)
        }
    }
}

@Composable
fun LowkeyTextField(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    placeholder: String = "",
    isPassword: Boolean = false,
    showPassword: Boolean = false,
    onTogglePassword: (() -> Unit)? = null,
    keyboardType: KeyboardType = KeyboardType.Text
) {
    OutlinedTextField(
        value = value,
        onValueChange = onValueChange,
        label = { Text(label, color = MutedColor) },
        placeholder = { Text(placeholder, color = MutedColor.copy(alpha = 0.5f)) },
        modifier = Modifier.fillMaxWidth(),
        singleLine = true,
        visualTransformation = if (isPassword && !showPassword) PasswordVisualTransformation() else VisualTransformation.None,
        keyboardOptions = KeyboardOptions(keyboardType = keyboardType),
        trailingIcon = if (isPassword && onTogglePassword != null) {
            {
                IconButton(onClick = onTogglePassword) {
                    Icon(
                        if (showPassword) Icons.Default.VisibilityOff else Icons.Default.Visibility,
                        null, tint = MutedColor
                    )
                }
            }
        } else null,
        colors = OutlinedTextFieldDefaults.colors(
            focusedBorderColor = GreenColor,
            unfocusedBorderColor = Color(0xFF1A1A3E),
            focusedTextColor = TextColor,
            unfocusedTextColor = TextColor,
            cursorColor = GreenColor,
            focusedLabelColor = GreenColor
        ),
        shape = RoundedCornerShape(10.dp)
    )
}

// ── Utilities ─────────────────────────────────────────────────────────────────

private val dateFormatter = DateTimeFormatter.ofPattern("dd.MM.yyyy")
    .withZone(ZoneId.systemDefault())

fun formatDate(isoStr: String): String = try {
    dateFormatter.format(Instant.parse(isoStr))
} catch (_: Exception) { isoStr }

fun daysLeft(isoStr: String): Int = try {
    val epochMs = Instant.parse(isoStr).toEpochMilli()
    maxOf(0, ((epochMs - System.currentTimeMillis()) / 86_400_000).toInt())
} catch (_: Exception) { 0 }

fun dayLabel(n: Int) = when {
    n % 10 == 1 && n % 100 != 11 -> "день"
    n % 10 in 2..4 && n % 100 !in 12..14 -> "дня"
    else -> "дней"
}

// ── Root entry point with modals ──────────────────────────────────────────────

@Composable
fun LowkeyAppWithModal(vm: MainViewModel = viewModel()) {
    LowkeyApp(vm = vm)
}
