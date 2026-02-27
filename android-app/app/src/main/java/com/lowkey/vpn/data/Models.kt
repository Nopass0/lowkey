package com.lowkey.vpn.data

import com.google.gson.annotations.SerializedName

// ── Auth ──────────────────────────────────────────────────────────────────────

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

// ── User ──────────────────────────────────────────────────────────────────────

data class UserModel(
    val id: Int,
    val login: String,
    val balance: Double,
    /** "active" | "inactive" | "expired" */
    @SerializedName("sub_status") val subStatus: String?,
    /** ISO-8601 date string e.g. "2024-01-15T12:00:00Z" */
    @SerializedName("sub_expires_at") val subExpiresAt: String?,
    /** Bandwidth cap in Mbit/s; 0 = unlimited */
    @SerializedName("sub_speed_mbps") val subSpeedMbps: Double = 0.0,
    @SerializedName("referral_code") val referralCode: String?,
    @SerializedName("referral_balance") val referralBalance: Double,
    @SerializedName("first_purchase_done") val firstPurchaseDone: Boolean,
    val role: String = "user"
)

// ── Subscription plans ────────────────────────────────────────────────────────

data class PlanModel(
    @SerializedName("plan_key") val planKey: String = "",
    val name: String,
    @SerializedName("price_rub") val priceRub: Double,
    @SerializedName("duration_days") val durationDays: Int,
    @SerializedName("speed_mbps") val speedMbps: Double,
    @SerializedName("is_bundle") val isBundle: Boolean = false,
    @SerializedName("discount_pct") val discountPct: Int = 0,
    val description: String? = null
)

/** Wrapper returned by GET /subscription/plans */
data class PlansResponse(val plans: List<PlanModel>)

// ── Payments (SBP) ────────────────────────────────────────────────────────────

data class CreatePaymentRequest(
    val amount: Double,
    /** "balance" or "subscription" */
    val purpose: String,
    @SerializedName("plan_id") val planId: String? = null
)

data class CreatePaymentResponse(
    @SerializedName("payment_id") val paymentId: Int,
    @SerializedName("qr_payload") val qrPayload: String,
    @SerializedName("qr_url") val qrUrl: String?,
    val amount: Double,
    @SerializedName("expires_at") val expiresAt: String?
)

data class PaymentStatusResponse(
    @SerializedName("payment_id") val paymentId: Int,
    /** "pending" | "paid" | "expired" | "failed" */
    val status: String,
    val amount: Double,
    @SerializedName("paid_at") val paidAt: String?,
    @SerializedName("balance_after") val balanceAfter: Double?,
    @SerializedName("sub_expires_at") val subExpiresAt: String?
)

data class PaymentHistoryResponse(val payments: List<PaymentItem>)

data class PaymentItem(
    val id: Int,
    val amount: Double,
    val purpose: String,
    @SerializedName("plan_id") val planId: String?,
    val status: String,
    @SerializedName("created_at") val createdAt: String,
    @SerializedName("paid_at") val paidAt: String?
)

// ── Promo codes ────────────────────────────────────────────────────────────────

data class ApplyPromoRequest(val code: String)

data class ApplyPromoResponse(
    val message: String,
    @SerializedName("new_balance") val newBalance: Double,
    @SerializedName("sub_expires_at") val subExpiresAt: String?
)

// ── Referral ──────────────────────────────────────────────────────────────────

data class ReferralStatsModel(
    @SerializedName("referral_code") val referralCode: String?,
    @SerializedName("referral_count") val referralCount: Int,
    @SerializedName("referral_balance") val referralBalance: Double,
    @SerializedName("total_earned") val totalEarned: Double
)

data class WithdrawRequest(
    val amount: Double,
    @SerializedName("card_number") val cardNumber: String,
    @SerializedName("bank_name") val bankName: String? = null
)

data class WithdrawResponse(
    @SerializedName("withdrawal_id") val withdrawalId: Int,
    val status: String,
    val message: String
)

data class WithdrawalModel(
    val id: Int,
    val amount: Double,
    /** "pending" | "processing" | "completed" | "rejected" */
    val status: String,
    @SerializedName("card_number") val cardNumber: String = "",
    @SerializedName("bank_name") val bankName: String?,
    @SerializedName("admin_note") val adminNote: String?,
    @SerializedName("requested_at") val requestedAt: String = "",
    @SerializedName("processed_at") val processedAt: String?
)

data class WithdrawalsResponse(val withdrawals: List<WithdrawalModel>)

// ── VPN registration ──────────────────────────────────────────────────────────

/**
 * Response from POST /api/peers/register.
 * The server assigns a VPN IP (10.0.0.x) and returns connection params.
 */
data class VpnCredentials(
    /** Assigned VPN IP, e.g. "10.0.0.5" */
    @SerializedName("vpn_ip") val vpnIp: String,
    /** Server's VPN-side IP, always "10.0.0.1" */
    @SerializedName("server_vpn_ip") val serverVpnIp: String = "10.0.0.1",
    /** Server UDP port for the tunnel */
    @SerializedName("udp_port") val udpPort: Int,
    /** Pre-shared key used for encrypting the UDP tunnel */
    val psk: String
)

/** Local-only VPN connection status (not fetched from server). */
data class VpnStatus(
    val connected: Boolean,
    val vpnIp: String? = null
)

// ── App release / auto-update ─────────────────────────────────────────────────

data class AppReleaseInfo(
    val id: Int,
    val version: String,
    @SerializedName("download_url") val downloadUrl: String,
    @SerializedName("file_name") val fileName: String?,
    @SerializedName("changelog") val changelog: String?,
    @SerializedName("released_at") val releasedAt: String
)

// ── Error response ────────────────────────────────────────────────────────────

data class ApiError(val error: String)
