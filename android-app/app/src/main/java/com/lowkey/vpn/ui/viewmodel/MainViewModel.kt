package com.lowkey.vpn.ui.viewmodel

import android.app.Application
import android.content.Intent
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.lowkey.vpn.BuildConfig
import com.lowkey.vpn.data.AppReleaseInfo
import com.lowkey.vpn.data.LowkeyApiService
import com.lowkey.vpn.data.PaymentItem
import com.lowkey.vpn.data.PlanModel
import com.lowkey.vpn.data.ReferralStatsModel
import com.lowkey.vpn.data.UserModel
import com.lowkey.vpn.data.WithdrawalModel
import com.lowkey.vpn.vpn.LowkeyVpnService
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch

/**
 * Main ViewModel — manages auth state, subscription data and VPN connection.
 *
 * UI state is exposed as read-only [StateFlow]s.  All network calls run inside
 * [viewModelScope] so they are automatically cancelled on ViewModel clear.
 */
class MainViewModel(application: Application) : AndroidViewModel(application) {

    private val api = LowkeyApiService(application)

    // ── Auth ──────────────────────────────────────────────────────────────────

    private val _token = MutableStateFlow<String?>(null)
    val token: StateFlow<String?> = _token

    private val _user = MutableStateFlow<UserModel?>(null)
    val user: StateFlow<UserModel?> = _user

    // ── VPN ───────────────────────────────────────────────────────────────────

    private val _connected = MutableStateFlow(false)
    val connected: StateFlow<Boolean> = _connected

    private val _toggling = MutableStateFlow(false)
    val toggling: StateFlow<Boolean> = _toggling

    // ── Subscription & data ───────────────────────────────────────────────────

    private val _plans = MutableStateFlow<List<PlanModel>>(emptyList())
    val plans: StateFlow<List<PlanModel>> = _plans

    private val _refStats = MutableStateFlow<ReferralStatsModel?>(null)
    val refStats: StateFlow<ReferralStatsModel?> = _refStats

    private val _payHistory = MutableStateFlow<List<PaymentItem>>(emptyList())
    val payHistory: StateFlow<List<PaymentItem>> = _payHistory

    private val _withdrawals = MutableStateFlow<List<WithdrawalModel>>(emptyList())
    val withdrawals: StateFlow<List<WithdrawalModel>> = _withdrawals

    // ── Auto-update ───────────────────────────────────────────────────────────

    private val _updateAvailable = MutableStateFlow<AppReleaseInfo?>(null)
    val updateAvailable: StateFlow<AppReleaseInfo?> = _updateAvailable

    // ── Loading / messages ────────────────────────────────────────────────────

    private val _isLoading = MutableStateFlow(false)
    val isLoading: StateFlow<Boolean> = _isLoading

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error

    private val _successMsg = MutableStateFlow<String?>(null)
    val successMsg: StateFlow<String?> = _successMsg

    // ── Payment modal ─────────────────────────────────────────────────────────

    private val _showPayModal = MutableStateFlow(false)
    val showPayModal: StateFlow<Boolean> = _showPayModal

    private val _paymentQrUrl = MutableStateFlow<String?>(null)
    val paymentQrUrl: StateFlow<String?> = _paymentQrUrl

    private val _currentPaymentId = MutableStateFlow<Int?>(null)
    val currentPaymentId: StateFlow<Int?> = _currentPaymentId

    private val _paymentStatus = MutableStateFlow<String?>(null)
    val paymentStatus: StateFlow<String?> = _paymentStatus

    init {
        val savedToken = api.token
        if (savedToken != null) {
            _token.value = savedToken
            viewModelScope.launch { loadUserData() }
        }
    }

    // ── Auth ──────────────────────────────────────────────────────────────────

    fun login(login: String, password: String) {
        viewModelScope.launch {
            _isLoading.value = true
            _error.value = null
            api.login(login, password)
                .onSuccess { auth ->
                    api.token = auth.token
                    _token.value = auth.token
                    _user.value = auth.user
                    loadUserData()
                }
                .onFailure { _error.value = it.message ?: "Ошибка входа" }
            _isLoading.value = false
        }
    }

    fun register(login: String, password: String, referralCode: String?) {
        viewModelScope.launch {
            _isLoading.value = true
            _error.value = null
            api.register(login, password, referralCode.takeIf { !it.isNullOrBlank() })
                .onSuccess { auth ->
                    api.token = auth.token
                    _token.value = auth.token
                    _user.value = auth.user
                    loadUserData()
                }
                .onFailure { _error.value = it.message ?: "Ошибка регистрации" }
            _isLoading.value = false
        }
    }

    fun logout() {
        api.token = null
        _token.value = null
        _user.value = null
        _connected.value = false
        _plans.value = emptyList()
        _refStats.value = null
        _payHistory.value = emptyList()
        _withdrawals.value = emptyList()
        val ctx = getApplication<Application>()
        ctx.stopService(Intent(ctx, LowkeyVpnService::class.java))
    }

    // ── Data loading ──────────────────────────────────────────────────────────

    private suspend fun loadUserData() {
        api.me().onSuccess { _user.value = it }
        api.getPlans().onSuccess { _plans.value = it }
        api.getReferralStats().onSuccess { _refStats.value = it }
        api.getPaymentHistory().onSuccess { _payHistory.value = it.payments }
        api.getWithdrawals().onSuccess { _withdrawals.value = it.withdrawals }
    }

    fun refreshAll() {
        viewModelScope.launch { loadUserData() }
    }

    // ── VPN ───────────────────────────────────────────────────────────────────

    fun toggleVpn() {
        viewModelScope.launch {
            if (_toggling.value) return@launch
            _toggling.value = true
            val ctx = getApplication<Application>()

            if (_connected.value) {
                ctx.stopService(Intent(ctx, LowkeyVpnService::class.java))
                delay(500)
                _connected.value = false
            } else {
                api.registerVpn()
                    .onSuccess { creds ->
                        // Extract hostname from the configured API URL
                        val host = api.apiUrl
                            .removePrefix("https://")
                            .removePrefix("http://")
                            .split(":")[0]
                            .split("/")[0]

                        val intent = Intent(ctx, LowkeyVpnService::class.java).apply {
                            putExtra("host",   host)
                            putExtra("port",   creds.udpPort)
                            putExtra("psk",    creds.psk)
                            putExtra("vpn_ip", creds.vpnIp)
                        }
                        ctx.startService(intent)
                        delay(1000)
                        _connected.value = true
                    }
                    .onFailure { _error.value = it.message ?: "Ошибка подключения" }
            }
            _toggling.value = false
        }
    }

    // ── Payment modal ─────────────────────────────────────────────────────────

    fun openPayModal() {
        _showPayModal.value     = true
        _paymentQrUrl.value     = null
        _currentPaymentId.value = null
        _paymentStatus.value    = null
    }

    fun closePayModal() {
        _showPayModal.value     = false
        _paymentQrUrl.value     = null
        _currentPaymentId.value = null
        _paymentStatus.value    = null
    }

    fun createPayment(amount: Double, purpose: String, planKey: String? = null) {
        viewModelScope.launch {
            _isLoading.value = true
            _error.value = null
            api.createSbpPayment(amount, purpose, planKey)
                .onSuccess { payment ->
                    _paymentQrUrl.value     = payment.qrUrl
                    _currentPaymentId.value = payment.paymentId
                    _paymentStatus.value    = "pending"
                    startPollingPayment(payment.paymentId)
                }
                .onFailure { _error.value = it.message ?: "Ошибка создания платежа" }
            _isLoading.value = false
        }
    }

    private fun startPollingPayment(paymentId: Int) {
        viewModelScope.launch {
            repeat(60) {
                delay(2500)
                api.getPaymentStatus(paymentId)
                    .onSuccess { s ->
                        _paymentStatus.value = s.status
                        if (s.status == "paid") {
                            loadUserData()
                            return@launch
                        }
                    }
                if (_paymentStatus.value != "pending") return@launch
            }
        }
    }

    // ── Promo codes ────────────────────────────────────────────────────────────

    fun applyPromo(code: String) {
        viewModelScope.launch {
            _isLoading.value = true
            _error.value = null
            _successMsg.value = null
            api.applyPromo(code)
                .onSuccess { resp ->
                    _successMsg.value = resp.message
                    loadUserData()
                }
                .onFailure { _error.value = it.message ?: "Неверный промокод" }
            _isLoading.value = false
        }
    }

    // ── Referral withdrawal ───────────────────────────────────────────────────

    fun requestWithdrawal(amount: Double, cardNumber: String, bankName: String?) {
        viewModelScope.launch {
            _isLoading.value = true
            _error.value = null
            _successMsg.value = null
            api.requestWithdrawal(amount, cardNumber, bankName)
                .onSuccess { resp ->
                    _successMsg.value = "Заявка #${resp.withdrawalId} создана. Ожидайте подтверждения."
                    api.getWithdrawals().onSuccess { _withdrawals.value = it.withdrawals }
                    api.getReferralStats().onSuccess { _refStats.value = it }
                }
                .onFailure { _error.value = it.message ?: "Ошибка вывода" }
            _isLoading.value = false
        }
    }

    // ── Auto-update ───────────────────────────────────────────────────────────

    /**
     * Check if a newer Android release is available on the server.
     * Skipped in debug builds to avoid annoying developers.
     */
    fun checkForUpdate(currentVersion: String) {
        if (BuildConfig.DEBUG) return
        viewModelScope.launch {
            api.getLatestRelease("android").onSuccess { release ->
                if (compareVersions(release.version, currentVersion) > 0) {
                    _updateAvailable.value = release
                }
            }
        }
    }

    fun dismissUpdate() { _updateAvailable.value = null }

    private fun compareVersions(v1: String, v2: String): Int {
        val p1 = v1.split(".").map { it.toIntOrNull() ?: 0 }
        val p2 = v2.split(".").map { it.toIntOrNull() ?: 0 }
        for (i in 0 until maxOf(p1.size, p2.size)) {
            val diff = (p1.getOrElse(i) { 0 }).compareTo(p2.getOrElse(i) { 0 })
            if (diff != 0) return diff
        }
        return 0
    }

    // ── Misc ──────────────────────────────────────────────────────────────────

    fun clearError()   { _error.value = null }
    fun clearSuccess() { _successMsg.value = null }
}
