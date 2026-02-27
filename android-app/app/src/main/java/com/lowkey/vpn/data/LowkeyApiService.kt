package com.lowkey.vpn.data

import android.content.Context
import android.content.SharedPreferences
import com.google.gson.Gson
import com.google.gson.reflect.TypeToken
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.BufferedReader
import java.io.InputStreamReader
import java.io.OutputStreamWriter
import java.net.HttpURLConnection
import java.net.URL

/**
 * HTTP API client for Lowkey VPN backend.
 *
 * All network requests run on the IO dispatcher.  Auth token and base URL
 * are persisted in SharedPreferences so they survive process restarts.
 */
class LowkeyApiService(context: Context) {

    private val prefs: SharedPreferences =
        context.getSharedPreferences("lowkey_prefs", Context.MODE_PRIVATE)
    private val gson = Gson()

    companion object {
        const val DEFAULT_API_URL = "https://api.lowkeyvpn.com"
        const val PREF_API_URL   = "api_url"
        const val PREF_TOKEN     = "auth_token"
    }

    var apiUrl: String
        get() = prefs.getString(PREF_API_URL, DEFAULT_API_URL) ?: DEFAULT_API_URL
        set(value) { prefs.edit().putString(PREF_API_URL, value).apply() }

    var token: String?
        get() = prefs.getString(PREF_TOKEN, null)
        set(value) {
            if (value == null) prefs.edit().remove(PREF_TOKEN).apply()
            else prefs.edit().putString(PREF_TOKEN, value).apply()
        }

    // ── Low-level HTTP helper ─────────────────────────────────────────────────

    private suspend fun <T> request(
        method: String,
        path: String,
        body: Any? = null,
        typeToken: TypeToken<T>,
        auth: Boolean = true
    ): Result<T> = withContext(Dispatchers.IO) {
        try {
            val url  = URL("$apiUrl$path")
            val conn = url.openConnection() as HttpURLConnection
            conn.requestMethod = method
            conn.setRequestProperty("Content-Type", "application/json")
            conn.setRequestProperty("Accept", "application/json")
            if (auth && token != null) {
                conn.setRequestProperty("Authorization", "Bearer $token")
            }
            conn.connectTimeout = 10_000
            conn.readTimeout    = 15_000

            if (body != null) {
                conn.doOutput = true
                OutputStreamWriter(conn.outputStream).use { w ->
                    w.write(gson.toJson(body))
                }
            }

            val code   = conn.responseCode
            val stream = if (code in 200..299) conn.inputStream else conn.errorStream
            val response = BufferedReader(InputStreamReader(stream)).use { it.readText() }

            if (code in 200..299) {
                Result.success(gson.fromJson<T>(response, typeToken.type))
            } else {
                val errMsg = try {
                    gson.fromJson(response, ApiError::class.java).error
                } catch (_: Exception) {
                    response.take(200).ifBlank { "HTTP $code" }
                }
                Result.failure(Exception(errMsg))
            }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    // ── Typed convenience wrappers ────────────────────────────────────────────

    private suspend inline fun <reified T> get(path: String, auth: Boolean = true) =
        request<T>("GET", path, null, object : TypeToken<T>() {}, auth)

    private suspend inline fun <reified T> post(path: String, body: Any? = null, auth: Boolean = true) =
        request<T>("POST", path, body, object : TypeToken<T>() {}, auth)

    // ── Auth ──────────────────────────────────────────────────────────────────

    suspend fun login(login: String, password: String): Result<AuthResponse> =
        post("/auth/login", LoginRequest(login, password), auth = false)

    suspend fun register(login: String, password: String, referralCode: String?): Result<AuthResponse> =
        post("/auth/register", RegisterRequest(login, password, referralCode), auth = false)

    suspend fun me(): Result<UserModel> = get("/auth/me")

    // ── Subscription plans ────────────────────────────────────────────────────

    /** Returns the list of subscription plans. The server wraps them: { "plans": [...] }. */
    suspend fun getPlans(): Result<List<PlanModel>> =
        get<PlansResponse>("/subscription/plans", auth = false).map { it.plans }

    // ── Payments (SBP) ────────────────────────────────────────────────────────

    /**
     * Create an SBP QR payment order.
     *
     * @param amount    Amount in rubles (min 10, max 100 000).
     * @param purpose   "balance" to top-up wallet, "subscription" to buy a plan.
     * @param planKey   Required when purpose = "subscription".
     */
    suspend fun createSbpPayment(
        amount: Double,
        purpose: String,
        planKey: String? = null
    ): Result<CreatePaymentResponse> =
        post("/payment/sbp/create", CreatePaymentRequest(amount, purpose, planKey))

    /** Poll SBP payment status by numeric ID. */
    suspend fun getPaymentStatus(paymentId: Int): Result<PaymentStatusResponse> =
        get("/payment/sbp/status/$paymentId")

    /** Fetch the user's full payment history. */
    suspend fun getPaymentHistory(): Result<PaymentHistoryResponse> =
        get("/payment/history")

    // ── Promo codes ────────────────────────────────────────────────────────────

    /** Apply a promo code to the current user's account. */
    suspend fun applyPromo(code: String): Result<ApplyPromoResponse> =
        post("/promo/apply", ApplyPromoRequest(code))

    // ── Referral ──────────────────────────────────────────────────────────────

    suspend fun getReferralStats(): Result<ReferralStatsModel> = get("/referral/stats")

    /** Request a withdrawal of referral balance to a bank card via SBP. */
    suspend fun requestWithdrawal(
        amount: Double,
        cardNumber: String,
        bankName: String? = null
    ): Result<WithdrawResponse> =
        post("/referral/withdraw", WithdrawRequest(amount, cardNumber, bankName))

    /** List the current user's withdrawal requests. */
    suspend fun getWithdrawals(): Result<WithdrawalsResponse> = get("/referral/withdrawals")

    // ── App version / auto-update ─────────────────────────────────────────────

    /**
     * Fetch the latest release info for a given platform from the public API.
     * Does not require authentication.
     */
    suspend fun getLatestRelease(platform: String): Result<AppReleaseInfo> =
        get("/api/version/$platform", auth = false)

    // ── VPN peer registration ─────────────────────────────────────────────────

    /**
     * Register a VPN peer (POST /api/peers/register).
     *
     * Returns the assigned VPN IP, server UDP port and PSK needed to open
     * the encrypted UDP tunnel in [LowkeyVpnService].
     */
    suspend fun registerVpn(): Result<VpnCredentials> =
        post("/api/peers/register", emptyMap<String, String>())
}
