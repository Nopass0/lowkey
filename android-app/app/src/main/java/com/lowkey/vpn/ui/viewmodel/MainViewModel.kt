package com.lowkey.vpn.ui.viewmodel

import android.app.Application
import android.content.Intent
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.lowkey.vpn.data.LowkeyApiService
import com.lowkey.vpn.data.PlanModel
import com.lowkey.vpn.data.ReferralStatsModel
import com.lowkey.vpn.data.UserModel
import com.lowkey.vpn.vpn.LowkeyVpnService
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch

class MainViewModel(application: Application) : AndroidViewModel(application) {

    private val api = LowkeyApiService(application)

    private val _token = MutableStateFlow<String?>(null)
    val token: StateFlow<String?> = _token

    private val _user = MutableStateFlow<UserModel?>(null)
    val user: StateFlow<UserModel?> = _user

    private val _connected = MutableStateFlow(false)
    val connected: StateFlow<Boolean> = _connected

    private val _toggling = MutableStateFlow(false)
    val toggling: StateFlow<Boolean> = _toggling

    private val _plans = MutableStateFlow<List<PlanModel>>(emptyList())
    val plans: StateFlow<List<PlanModel>> = _plans

    private val _refStats = MutableStateFlow<ReferralStatsModel?>(null)
    val refStats: StateFlow<ReferralStatsModel?> = _refStats

    private val _isLoading = MutableStateFlow(false)
    val isLoading: StateFlow<Boolean> = _isLoading

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error

    // Payment modal state
    private val _showPayModal = MutableStateFlow(false)
    val showPayModal: StateFlow<Boolean> = _showPayModal

    private val _paymentQrUrl = MutableStateFlow<String?>(null)
    val paymentQrUrl: StateFlow<String?> = _paymentQrUrl

    private val _paymentId = MutableStateFlow<String?>(null)
    val paymentId: StateFlow<String?> = _paymentId

    private val _paymentStatus = MutableStateFlow<String?>(null)
    val paymentStatus: StateFlow<String?> = _paymentStatus

    init {
        // Restore token from prefs
        val savedToken = api.token
        if (savedToken != null) {
            _token.value = savedToken
            viewModelScope.launch {
                loadUserData()
            }
        }
    }

    fun login(login: String, password: String) {
        viewModelScope.launch {
            _isLoading.value = true
            _error.value = null
            val result = api.login(login, password)
            result.onSuccess { auth ->
                api.token = auth.token
                _token.value = auth.token
                _user.value = auth.user
                loadUserData()
            }.onFailure { e ->
                _error.value = e.message ?: "Ошибка входа"
            }
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
        // Stop VPN if connected
        val ctx = getApplication<Application>()
        ctx.stopService(Intent(ctx, LowkeyVpnService::class.java))
    }

    private suspend fun loadUserData() {
        // Load user info
        api.me().onSuccess { u ->
            _user.value = u
        }
        // Load plans
        api.getPlans().onSuccess { p ->
            _plans.value = p
        }
        // Load referral stats
        api.getReferralStats().onSuccess { stats ->
            _refStats.value = stats
        }
        // Check VPN status
        api.getVpnStatus().onSuccess { status ->
            _connected.value = status.connected
        }
    }

    fun toggleVpn() {
        viewModelScope.launch {
            if (_toggling.value) return@launch
            _toggling.value = true
            val ctx = getApplication<Application>()

            if (_connected.value) {
                // Disconnect
                ctx.stopService(Intent(ctx, LowkeyVpnService::class.java))
                delay(500)
                _connected.value = false
            } else {
                // Connect - register VPN session
                val result = api.registerVpn()
                result.onSuccess { creds ->
                    val intent = Intent(ctx, LowkeyVpnService::class.java).apply {
                        putExtra("host", creds.host)
                        putExtra("port", creds.port)
                        putExtra("token", creds.token)
                        putExtra("vpn_ip", creds.vpnIp)
                    }
                    ctx.startService(intent)
                    delay(1000)
                    _connected.value = true
                }.onFailure { e ->
                    _error.value = e.message ?: "Ошибка подключения"
                }
            }
            _toggling.value = false
        }
    }

    fun openPayModal() {
        _showPayModal.value = true
        _paymentQrUrl.value = null
        _paymentId.value = null
        _paymentStatus.value = null
    }

    fun closePayModal() {
        _showPayModal.value = false
        _paymentQrUrl.value = null
        _paymentId.value = null
        _paymentStatus.value = null
    }

    fun createPayment(amount: Double, paymentType: String, planKey: String? = null) {
        viewModelScope.launch {
            _isLoading.value = true
            _error.value = null
            val result = api.createSbpPayment(amount, paymentType, planKey)
            result.onSuccess { payment ->
                _paymentQrUrl.value = payment.qrUrl
                _paymentId.value = payment.paymentId
                _paymentStatus.value = "pending"
                startPollingPayment(payment.paymentId)
            }.onFailure { e ->
                _error.value = e.message ?: "Ошибка создания платежа"
            }
            _isLoading.value = false
        }
    }

    private fun startPollingPayment(paymentId: String) {
        viewModelScope.launch {
            repeat(60) { // Poll for up to 2.5 minutes
                delay(2500)
                val status = api.getPaymentStatus(paymentId)
                status.onSuccess { s ->
                    _paymentStatus.value = s.status
                    if (s.status == "paid") {
                        // Reload user data to update balance/subscription
                        loadUserData()
                        return@launch
                    }
                }
                if (_paymentStatus.value != "pending") return@launch
            }
        }
    }

    fun refreshRefStats() {
        viewModelScope.launch {
            api.getReferralStats().onSuccess { stats ->
                _refStats.value = stats
            }
        }
    }

    fun clearError() {
        _error.value = null
    }
}
