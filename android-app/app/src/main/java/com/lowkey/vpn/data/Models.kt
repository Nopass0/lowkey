package com.lowkey.vpn.data

import com.google.gson.annotations.SerializedName

data class LoginRequest(
    val login: String,
    val password: String
)

data class RegisterRequest(
    val login: String,
    val password: String,
    @SerializedName("referral_code") val referralCode: String? = null
)

data class AuthResponse(
    val token: String,
    val user: UserModel
)

data class UserModel(
    val id: Int,
    val login: String,
    val balance: Double,
    @SerializedName("sub_status") val subStatus: String?,
    @SerializedName("sub_expires_at") val subExpiresAt: Long?,
    @SerializedName("referral_code") val referralCode: String?,
    @SerializedName("referral_balance") val referralBalance: Double,
    @SerializedName("first_purchase_done") val firstPurchaseDone: Boolean,
    val banned: Boolean
)

data class PlanModel(
    @SerializedName("plan_key") val planKey: String,
    val name: String,
    @SerializedName("price_rub") val priceRub: Double,
    @SerializedName("duration_days") val durationDays: Int,
    @SerializedName("speed_mbps") val speedMbps: Double,
    val description: String?
)

data class ReferralStatsModel(
    @SerializedName("referral_code") val referralCode: String?,
    @SerializedName("referral_count") val referralCount: Int,
    @SerializedName("referral_balance") val referralBalance: Double,
    @SerializedName("total_earned") val totalEarned: Double,
    val withdrawals: List<WithdrawalModel>
)

data class WithdrawalModel(
    val id: Int,
    val amount: Double,
    val status: String,
    @SerializedName("created_at") val createdAt: Long,
    val note: String?
)

data class CreatePaymentRequest(
    val amount: Double,
    @SerializedName("payment_type") val paymentType: String,
    @SerializedName("plan_key") val planKey: String? = null
)

data class CreatePaymentResponse(
    @SerializedName("payment_id") val paymentId: String,
    @SerializedName("qr_url") val qrUrl: String,
    val amount: Double,
    val description: String
)

data class PaymentStatusResponse(
    @SerializedName("payment_id") val paymentId: String,
    val status: String,
    val amount: Double
)

data class WithdrawRequest(
    val amount: Double,
    @SerializedName("card_number") val cardNumber: String
)

data class ApiError(
    val error: String
)

data class VpnCredentials(
    val host: String,
    val port: Int,
    val token: String,
    @SerializedName("vpn_ip") val vpnIp: String
)

data class VpnStatus(
    val connected: Boolean,
    @SerializedName("vpn_ip") val vpnIp: String?
)
