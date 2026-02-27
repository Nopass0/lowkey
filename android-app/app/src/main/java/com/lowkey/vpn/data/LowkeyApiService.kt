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

class LowkeyApiService(context: Context) {

    private val prefs: SharedPreferences =
        context.getSharedPreferences("lowkey_prefs", Context.MODE_PRIVATE)
    private val gson = Gson()

    companion object {
        const val DEFAULT_API_URL = "https://api.lowkeyvpn.com"
        const val PREF_API_URL = "api_url"
        const val PREF_TOKEN = "auth_token"
    }

    var apiUrl: String
        get() = prefs.getString(PREF_API_URL, DEFAULT_API_URL) ?: DEFAULT_API_URL
        set(value) = prefs.edit().putString(PREF_API_URL, value).apply()

    var token: String?
        get() = prefs.getString(PREF_TOKEN, null)
        set(value) {
            if (value == null) prefs.edit().remove(PREF_TOKEN).apply()
            else prefs.edit().putString(PREF_TOKEN, value).apply()
        }

    private suspend fun <T> request(
        method: String,
        path: String,
        body: Any? = null,
        typeToken: TypeToken<T>,
        auth: Boolean = true
    ): Result<T> = withContext(Dispatchers.IO) {
        try {
            val url = URL("$apiUrl$path")
            val conn = url.openConnection() as HttpURLConnection
            conn.requestMethod = method
            conn.setRequestProperty("Content-Type", "application/json")
            conn.setRequestProperty("Accept", "application/json")
            if (auth && token != null) {
                conn.setRequestProperty("Authorization", "Bearer $token")
            }
            conn.connectTimeout = 10_000
            conn.readTimeout = 15_000

            if (body != null) {
                conn.doOutput = true
                val writer = OutputStreamWriter(conn.outputStream)
                writer.write(gson.toJson(body))
                writer.flush()
                writer.close()
            }

            val responseCode = conn.responseCode
            val stream = if (responseCode in 200..299) conn.inputStream else conn.errorStream
            val reader = BufferedReader(InputStreamReader(stream))
            val response = reader.readText()
            reader.close()

            if (responseCode in 200..299) {
                val result = gson.fromJson<T>(response, typeToken.type)
                Result.success(result)
            } else {
                val error = try {
                    gson.fromJson(response, ApiError::class.java).error
                } catch (e: Exception) {
                    "HTTP $responseCode"
                }
                Result.failure(Exception(error))
            }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    private suspend inline fun <reified T> get(path: String, auth: Boolean = true) =
        request<T>(
            "GET", path, null,
            object : TypeToken<T>() {}, auth
        )

    private suspend inline fun <reified T> post(path: String, body: Any? = null, auth: Boolean = true) =
        request<T>(
            "POST", path, body,
            object : TypeToken<T>() {}, auth
        )

    suspend fun login(login: String, password: String): Result<AuthResponse> =
        post("/auth/login", LoginRequest(login, password), auth = false)

    suspend fun register(login: String, password: String, referralCode: String?): Result<AuthResponse> =
        post("/auth/register", RegisterRequest(login, password, referralCode), auth = false)

    suspend fun me(): Result<UserModel> = get("/auth/me")

    suspend fun getPlans(): Result<List<PlanModel>> {
        val type = object : TypeToken<List<PlanModel>>() {}
        return request("GET", "/subscription/plans", null, type, auth = false)
    }

    suspend fun createSbpPayment(amount: Double, paymentType: String, planKey: String? = null): Result<CreatePaymentResponse> =
        post("/payment/sbp/create", CreatePaymentRequest(amount, paymentType, planKey))

    suspend fun getPaymentStatus(paymentId: String): Result<PaymentStatusResponse> =
        get("/payment/sbp/status/$paymentId")

    suspend fun getReferralStats(): Result<ReferralStatsModel> = get("/referral/stats")

    suspend fun requestWithdrawal(amount: Double, cardNumber: String): Result<Map<String, String>> {
        val type = object : TypeToken<Map<String, String>>() {}
        return request("POST", "/referral/withdraw", WithdrawRequest(amount, cardNumber), type)
    }

    suspend fun registerVpn(): Result<VpnCredentials> = post("/vpn/register")

    suspend fun getVpnStatus(): Result<VpnStatus> = get("/vpn/status")
}
