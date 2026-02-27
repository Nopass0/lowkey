package com.lowkey.vpn.ui

import android.content.Intent
import android.net.Uri
import androidx.compose.animation.*
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.lowkey.vpn.ui.viewmodel.MainViewModel

// Brand Colors
val BgColor = Color(0xFF06060F)
val CardColor = Color(0xFF0D0D1F)
val GreenColor = Color(0xFF00FF88)
val BlueColor = Color(0xFF0066FF)
val TextColor = Color(0xFFF0F4FF)
val MutedColor = Color(0xFF8892B0)
val DangerColor = Color(0xFFFF4444)

@Composable
fun LowkeyApp(vm: MainViewModel = viewModel()) {
    val token by vm.token.collectAsState()

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(BgColor)
    ) {
        AnimatedContent(targetState = token != null) { isLoggedIn ->
            if (isLoggedIn) {
                MainScreen(vm = vm)
            } else {
                LoginScreen(vm = vm)
            }
        }
    }
}

@Composable
fun LoginScreen(vm: MainViewModel) {
    var login by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    val isLoading by vm.isLoading.collectAsState()
    val error by vm.error.collectAsState()

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        // Logo
        Box(
            modifier = Modifier
                .size(72.dp)
                .background(
                    brush = Brush.linearGradient(listOf(GreenColor, BlueColor)),
                    shape = RoundedCornerShape(18.dp)
                ),
            contentAlignment = Alignment.Center
        ) {
            Icon(Icons.Default.Lock, contentDescription = null, tint = Color.Black, modifier = Modifier.size(36.dp))
        }

        Spacer(Modifier.height(20.dp))
        Text("Lowkey VPN", fontSize = 26.sp, fontWeight = FontWeight.Bold, color = GreenColor)
        Text("Безопасный и быстрый VPN", fontSize = 13.sp, color = MutedColor)

        Spacer(Modifier.height(36.dp))

        error?.let {
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(containerColor = DangerColor.copy(alpha = 0.1f)),
                shape = RoundedCornerShape(12.dp)
            ) {
                Text(it, modifier = Modifier.padding(12.dp), color = DangerColor, fontSize = 13.sp)
            }
            Spacer(Modifier.height(12.dp))
        }

        OutlinedTextField(
            value = login,
            onValueChange = { login = it },
            label = { Text("Логин") },
            modifier = Modifier.fillMaxWidth(),
            colors = OutlinedTextFieldDefaults.colors(
                focusedBorderColor = GreenColor,
                unfocusedBorderColor = Color(0xFF1A1A3E),
                focusedTextColor = TextColor,
                unfocusedTextColor = TextColor,
                cursorColor = GreenColor,
            )
        )

        Spacer(Modifier.height(12.dp))

        OutlinedTextField(
            value = password,
            onValueChange = { password = it },
            label = { Text("Пароль") },
            visualTransformation = PasswordVisualTransformation(),
            modifier = Modifier.fillMaxWidth(),
            colors = OutlinedTextFieldDefaults.colors(
                focusedBorderColor = GreenColor,
                unfocusedBorderColor = Color(0xFF1A1A3E),
                focusedTextColor = TextColor,
                unfocusedTextColor = TextColor,
                cursorColor = GreenColor,
            )
        )

        Spacer(Modifier.height(20.dp))

        Button(
            onClick = { vm.login(login, password) },
            enabled = !isLoading && login.isNotBlank() && password.isNotBlank(),
            modifier = Modifier.fillMaxWidth().height(52.dp),
            colors = ButtonDefaults.buttonColors(containerColor = GreenColor),
            shape = RoundedCornerShape(12.dp)
        ) {
            if (isLoading) {
                CircularProgressIndicator(Modifier.size(20.dp), color = Color.Black, strokeWidth = 2.dp)
            } else {
                Text("Войти", color = Color.Black, fontWeight = FontWeight.Bold)
            }
        }
    }
}

@Composable
fun MainScreen(vm: MainViewModel) {
    val user by vm.user.collectAsState()
    val connected by vm.connected.collectAsState()
    val toggling by vm.toggling.collectAsState()
    var selectedTab by remember { mutableIntStateOf(0) }
    val tabs = listOf("Главная", "Тарифы", "Рефералы")

    Column(Modifier.fillMaxSize()) {
        // Header
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .background(CardColor)
                .padding(horizontal = 16.dp, vertical = 12.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text("Lowkey VPN", fontSize = 18.sp, fontWeight = FontWeight.Bold, color = GreenColor)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(user?.login ?: "", fontSize = 13.sp, color = MutedColor)
                Spacer(Modifier.width(8.dp))
                IconButton(onClick = { vm.logout() }) {
                    Icon(Icons.Default.ExitToApp, contentDescription = "Выйти", tint = MutedColor)
                }
            }
        }

        // Tabs
        TabRow(selectedTabIndex = selectedTab, containerColor = CardColor, contentColor = GreenColor) {
            tabs.forEachIndexed { index, tab ->
                Tab(selected = selectedTab == index, onClick = { selectedTab = index },
                    text = { Text(tab, fontSize = 12.sp) })
            }
        }

        Box(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)) {
            when (selectedTab) {
                0 -> HomeTab(vm = vm, user = user, connected = connected, toggling = toggling)
                1 -> PlansTab(vm = vm)
                2 -> ReferralTab(vm = vm)
            }
        }
    }
}

@Composable
fun HomeTab(vm: MainViewModel, user: com.lowkey.vpn.data.UserModel?, connected: Boolean, toggling: Boolean) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Spacer(Modifier.height(24.dp))

        // VPN Toggle Button
        val buttonColor = if (connected) GreenColor else DangerColor
        val buttonLabel = if (toggling) "..." else if (connected) "Подключён" else "Отключён"

        Button(
            onClick = { if (!toggling) vm.toggleVpn() },
            modifier = Modifier.size(140.dp),
            shape = androidx.compose.foundation.shape.CircleShape,
            colors = ButtonDefaults.buttonColors(
                containerColor = buttonColor.copy(alpha = 0.15f)
            ),
            border = androidx.compose.foundation.BorderStroke(3.dp, buttonColor)
        ) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                if (toggling) {
                    CircularProgressIndicator(Modifier.size(32.dp), color = buttonColor, strokeWidth = 3.dp)
                } else {
                    Icon(
                        if (connected) Icons.Default.Wifi else Icons.Default.WifiOff,
                        contentDescription = null,
                        tint = buttonColor,
                        modifier = Modifier.size(40.dp)
                    )
                }
            }
        }

        Spacer(Modifier.height(12.dp))
        Text(buttonLabel, fontSize = 18.sp, fontWeight = FontWeight.Bold,
            color = if (connected) GreenColor else DangerColor)
        Text(if (connected) "Трафик защищён" else "Нажмите для подключения",
            fontSize = 13.sp, color = MutedColor)

        Spacer(Modifier.height(28.dp))

        // Stats
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            StatCard(
                modifier = Modifier.weight(1f),
                title = "Баланс",
                value = "${user?.balance?.toInt()} ₽",
                onClick = { vm.openPayModal() }
            )
            StatCard(
                modifier = Modifier.weight(1f),
                title = "Подписка",
                value = if (user?.sub_status == "active") "Активна" else "Неактивна",
                valueColor = if (user?.sub_status == "active") GreenColor else DangerColor
            )
        }

        // Expiry notification
        user?.sub_expires_at?.let { exp ->
            val daysLeft = ((java.util.Date(exp).time - System.currentTimeMillis()) / 86400000).toInt()
            if (daysLeft in 1..5) {
                Spacer(Modifier.height(12.dp))
                Card(
                    modifier = Modifier.fillMaxWidth(),
                    colors = CardDefaults.cardColors(containerColor = Color(0xFFFFAA00).copy(alpha = 0.1f)),
                    shape = RoundedCornerShape(12.dp)
                ) {
                    Row(Modifier.padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Default.Warning, contentDescription = null, tint = Color(0xFFFFAA00))
                        Spacer(Modifier.width(8.dp))
                        Text("Подписка истекает через $daysLeft дн.", color = Color(0xFFFFAA00), fontSize = 13.sp)
                    }
                }
            }
        }
    }
}

@Composable
fun StatCard(modifier: Modifier, title: String, value: String, valueColor: Color = TextColor, onClick: (() -> Unit)? = null) {
    Card(
        modifier = modifier,
        colors = CardDefaults.cardColors(containerColor = CardColor),
        shape = RoundedCornerShape(16.dp),
        onClick = { onClick?.invoke() }
    ) {
        Column(Modifier.padding(16.dp)) {
            Text(title, fontSize = 12.sp, color = MutedColor)
            Spacer(Modifier.height(4.dp))
            Text(value, fontSize = 18.sp, fontWeight = FontWeight.Bold, color = valueColor)
            if (onClick != null) {
                Spacer(Modifier.height(8.dp))
                Text("Пополнить СБП", fontSize = 11.sp, color = GreenColor)
            }
        }
    }
}

@Composable
fun PlansTab(vm: MainViewModel) {
    val plans by vm.plans.collectAsState()

    Column {
        Text("Тарифы", fontSize = 20.sp, fontWeight = FontWeight.Bold, color = TextColor)
        Spacer(Modifier.height(16.dp))
        plans.forEach { plan ->
            PlanCard(plan = plan, vm = vm)
            Spacer(Modifier.height(12.dp))
        }
    }
}

@Composable
fun PlanCard(plan: com.lowkey.vpn.data.PlanModel, vm: MainViewModel) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = CardColor),
        shape = RoundedCornerShape(16.dp)
    ) {
        Column(Modifier.padding(16.dp)) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text(plan.name, fontWeight = FontWeight.Bold, color = TextColor)
                Text("${plan.price_rub.toInt()} ₽", fontWeight = FontWeight.Bold, color = GreenColor, fontSize = 18.sp)
            }
            Text(
                "${if (plan.speed_mbps == 0.0) "∞" else plan.speed_mbps.toInt()} Мбит/с · ${plan.duration_days} дней",
                fontSize = 13.sp, color = MutedColor
            )
            Spacer(Modifier.height(12.dp))
            Button(
                onClick = { vm.createPayment(plan.price_rub, "subscription", plan.plan_key) },
                modifier = Modifier.fillMaxWidth(),
                colors = ButtonDefaults.outlinedButtonColors(contentColor = GreenColor),
                border = androidx.compose.foundation.BorderStroke(1.dp, GreenColor.copy(alpha = 0.5f)),
                shape = RoundedCornerShape(10.dp)
            ) {
                Text("Оплатить СБП", color = GreenColor)
            }
        }
    }
}

@Composable
fun ReferralTab(vm: MainViewModel) {
    val refStats by vm.refStats.collectAsState()

    Column {
        Text("Реферальная программа", fontSize = 20.sp, fontWeight = FontWeight.Bold, color = TextColor)
        Spacer(Modifier.height(16.dp))

        refStats?.let { stats ->
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(containerColor = CardColor),
                shape = RoundedCornerShape(16.dp)
            ) {
                Column(Modifier.padding(16.dp)) {
                    Text("Реферальный баланс", fontSize = 13.sp, color = MutedColor)
                    Text("${stats.referral_balance.toInt()} ₽", fontSize = 28.sp,
                        fontWeight = FontWeight.Bold, color = GreenColor)
                }
            }

            Spacer(Modifier.height(12.dp))

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Card(Modifier.weight(1f), colors = CardDefaults.cardColors(containerColor = CardColor),
                    shape = RoundedCornerShape(16.dp)) {
                    Column(Modifier.padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                        Text("${stats.referral_count}", fontSize = 24.sp, fontWeight = FontWeight.Bold, color = GreenColor)
                        Text("Приглашено", fontSize = 12.sp, color = MutedColor)
                    }
                }
                Card(Modifier.weight(1f), colors = CardDefaults.cardColors(containerColor = CardColor),
                    shape = RoundedCornerShape(16.dp)) {
                    Column(Modifier.padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                        Text("${stats.total_earned.toInt()} ₽", fontSize = 22.sp, fontWeight = FontWeight.Bold, color = GreenColor)
                        Text("Заработано", fontSize = 12.sp, color = MutedColor)
                    }
                }
            }

            Spacer(Modifier.height(12.dp))

            stats.referral_code?.let { code ->
                Card(
                    modifier = Modifier.fillMaxWidth(),
                    colors = CardDefaults.cardColors(containerColor = CardColor),
                    shape = RoundedCornerShape(16.dp)
                ) {
                    Column(Modifier.padding(16.dp)) {
                        Text("Ваш реферальный код", fontSize = 13.sp, color = MutedColor)
                        Spacer(Modifier.height(8.dp))
                        Text(code, fontSize = 22.sp, fontWeight = FontWeight.Bold, color = GreenColor,
                            fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace)
                        Spacer(Modifier.height(4.dp))
                        Text("Пригласите друга и получайте 25% с его платежей", fontSize = 12.sp, color = MutedColor)
                    }
                }
            }
        } ?: CircularProgressIndicator(color = GreenColor)
    }
}

@Composable
fun PaymentModal(vm: MainViewModel) {
    val showPayModal by vm.showPayModal.collectAsState()
    val qrUrl by vm.paymentQrUrl.collectAsState()
    val paymentStatus by vm.paymentStatus.collectAsState()
    val isLoading by vm.isLoading.collectAsState()
    val context = LocalContext.current

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
                        Spacer(Modifier.height(8.dp))
                        Icon(
                            Icons.Default.CheckCircle,
                            contentDescription = null,
                            tint = GreenColor,
                            modifier = Modifier.size(64.dp)
                        )
                        Spacer(Modifier.height(8.dp))
                        Text("Баланс пополнен", color = TextColor, fontSize = 15.sp)
                    }
                    isLoading -> {
                        CircularProgressIndicator(color = GreenColor)
                        Spacer(Modifier.height(8.dp))
                        Text("Создание платежа...", color = MutedColor, fontSize = 13.sp)
                    }
                    qrUrl != null -> {
                        Text(
                            "Нажмите кнопку ниже, чтобы перейти в банк для оплаты по СБП",
                            color = MutedColor, fontSize = 13.sp
                        )
                        Spacer(Modifier.height(12.dp))
                        Button(
                            onClick = {
                                val intent = Intent(Intent.ACTION_VIEW, Uri.parse(qrUrl))
                                context.startActivity(intent)
                            },
                            colors = ButtonDefaults.buttonColors(containerColor = GreenColor),
                            shape = RoundedCornerShape(10.dp),
                            modifier = Modifier.fillMaxWidth()
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
                            LinearProgressIndicator(
                                color = GreenColor,
                                trackColor = GreenColor.copy(alpha = 0.2f),
                                modifier = Modifier.fillMaxWidth()
                            )
                        }
                    }
                    else -> {
                        Text("Введите сумму для пополнения баланса", color = MutedColor, fontSize = 13.sp)
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

@Composable
fun LowkeyAppWithModal(vm: MainViewModel = viewModel()) {
    Box {
        LowkeyApp(vm = vm)
        PaymentModal(vm = vm)
    }
}
