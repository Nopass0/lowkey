// API client for Lowkey VPN backend

const API_BASE = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001';

interface FetchOptions extends RequestInit {
  token?: string;
}

async function apiFetch<T>(path: string, options: FetchOptions = {}): Promise<T> {
  const { token, ...rest } = options;
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(rest.headers as Record<string, string> || {}),
  };
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }

  const res = await fetch(`${API_BASE}${path}`, {
    ...rest,
    headers,
  });

  if (!res.ok) {
    const text = await res.text();
    throw new Error(text || `HTTP ${res.status}`);
  }

  const contentType = res.headers.get('content-type');
  if (contentType?.includes('application/json')) {
    return res.json();
  }
  return res.text() as unknown as T;
}

// ── Auth ──────────────────────────────────────────────────────────────────────

export interface UserPublic {
  id: number;
  login: string;
  balance: number;
  sub_status: 'active' | 'inactive' | 'expired';
  sub_expires_at: string | null;
  sub_speed_mbps: number;
  role: 'user' | 'admin' | 'banned';
  referral_code: string | null;
  referral_balance: number;
  first_purchase_done: boolean;
}

export interface AuthResponse {
  token: string;
  user: UserPublic;
}

export const auth = {
  register: (login: string, password: string, referralCode?: string): Promise<AuthResponse> =>
    apiFetch('/auth/register', {
      method: 'POST',
      body: JSON.stringify({ login, password, referral_code: referralCode || null }),
    }),

  login: (login: string, password: string): Promise<AuthResponse> =>
    apiFetch('/auth/login', {
      method: 'POST',
      body: JSON.stringify({ login, password }),
    }),

  me: (token: string): Promise<UserPublic> =>
    apiFetch('/auth/me', { token }),
};

// ── Subscription ──────────────────────────────────────────────────────────────

export interface SubscriptionPlan {
  id?: number;
  plan_key?: string;
  name: string;
  price_rub: number;
  duration_days: number;
  speed_mbps: number;
  is_bundle?: boolean;
  bundle_months?: number;
  discount_pct?: number;
}

export const subscription = {
  plans: (): Promise<{ plans: SubscriptionPlan[] }> =>
    apiFetch('/subscription/plans'),

  buy: (token: string, planId: string): Promise<{
    status: string;
    plan: string;
    expires_at: string;
    price_paid: number;
    balance_after: number;
    discount_applied: boolean;
  }> =>
    apiFetch('/subscription/buy', {
      method: 'POST',
      body: JSON.stringify({ plan_id: planId }),
      token,
    }),

  status: (token: string) =>
    apiFetch<{
      status: string;
      expires_at: string | null;
      speed_mbps: number;
      balance: number;
    }>('/subscription/status', { token }),
};

// ── Payments / SBP ───────────────────────────────────────────────────────────

export interface CreatePaymentResponse {
  payment_id: number;
  qr_payload: string;
  qr_url: string | null;
  amount: number;
  expires_at: string | null;
}

export interface PaymentStatusResponse {
  payment_id: number;
  status: 'pending' | 'paid' | 'expired' | 'failed';
  amount: number;
  paid_at: string | null;
  balance_after: number | null;
  sub_expires_at: string | null;
}

export const payments = {
  createSbp: (token: string, amount: number, purpose: 'balance' | 'subscription', planId?: string): Promise<CreatePaymentResponse> =>
    apiFetch('/payment/sbp/create', {
      method: 'POST',
      body: JSON.stringify({ amount, purpose, plan_id: planId || null }),
      token,
    }),

  status: (token: string, paymentId: number): Promise<PaymentStatusResponse> =>
    apiFetch(`/payment/sbp/status/${paymentId}`, { token }),

  history: (token: string) =>
    apiFetch<{ payments: Payment[] }>('/payment/history', { token }),
};

export interface Payment {
  id: number;
  amount: number;
  purpose: string;
  plan_id: string | null;
  status: string;
  created_at: string;
  paid_at: string | null;
}

// ── Promo ─────────────────────────────────────────────────────────────────────

export const promos = {
  apply: (token: string, code: string) =>
    apiFetch<{ message: string; new_balance: number; sub_expires_at: string | null }>('/promo/apply', {
      method: 'POST',
      body: JSON.stringify({ code }),
      token,
    }),
};

// ── Referral ─────────────────────────────────────────────────────────────────

export interface ReferralStats {
  referral_code: string | null;
  referral_count: number;
  total_earned: number;
  referral_balance: number;
}

export const referral = {
  stats: (token: string): Promise<ReferralStats> =>
    apiFetch('/referral/stats', { token }),

  withdraw: (token: string, amount: number, cardNumber: string, bankName?: string) =>
    apiFetch<{ withdrawal_id: number; status: string; message: string }>('/referral/withdraw', {
      method: 'POST',
      body: JSON.stringify({ amount, card_number: cardNumber, bank_name: bankName }),
      token,
    }),

  withdrawals: (token: string) =>
    apiFetch<{ withdrawals: Withdrawal[] }>('/referral/withdrawals', { token }),
};

export interface Withdrawal {
  id: number;
  amount: number;
  card_number: string;
  bank_name: string | null;
  status: string;
  admin_note: string | null;
  requested_at: string;
  processed_at: string | null;
}

// ── VPN API ───────────────────────────────────────────────────────────────────

export const vpn = {
  register: (token: string) =>
    apiFetch<{ vpn_ip: string; server_ip: string; port: number; psk: string }>('/api/peers/register', {
      method: 'POST',
      body: JSON.stringify({}),
      token,
    }),

  status: (token: string) =>
    apiFetch<{ connected_peers: number; server_pubkey: string; public_ip: string }>('/api/status', { token }),
};

// ── Admin ─────────────────────────────────────────────────────────────────────

export const admin = {
  requestCode: () =>
    apiFetch<{ status: string }>('/admin/request-code', { method: 'POST' }),

  verifyCode: (code: string): Promise<{ token: string }> =>
    apiFetch('/admin/verify-code', {
      method: 'POST',
      body: JSON.stringify({ code }),
    }),

  users: (token: string) =>
    apiFetch<{ users: UserPublic[]; total: number }>('/admin/users', { token }),

  stats: (token: string) =>
    apiFetch<{
      total_users: number;
      active_subscriptions: number;
      total_revenue_rub: number;
      pending_referral_payouts_rub: number;
      total_referral_balance_frozen_rub: number;
    }>('/admin/stats', { token }),

  payments: (token: string) =>
    apiFetch<{ payments: Payment[]; total: number }>('/admin/payments', { token }),

  confirmPayment: (token: string, paymentId: number) =>
    apiFetch(`/admin/payment/${paymentId}/confirm`, { method: 'POST', token }),

  createPromo: (token: string, data: {
    code?: string;
    type: string;
    value: number;
    extra?: number;
    max_uses?: number;
    expires_days?: number;
    target_user_id?: number | null;
    only_new_users?: boolean;
    min_purchase_rub?: number | null;
    second_type?: string | null;
    second_value?: number;
    max_uses_per_user?: number;
    description?: string;
  }) =>
    apiFetch('/admin/promos', { method: 'POST', body: JSON.stringify(data), token }),

  listPromos: (token: string) =>
    apiFetch<{ promos: any[] }>('/admin/promos/list', { token }),

  deletePromo: (token: string, id: number) =>
    apiFetch(`/admin/promos/${id}`, { method: 'DELETE', token }),

  setUserLimit: (token: string, userId: number, limitMbps: number) =>
    apiFetch(`/admin/users/${userId}/limit`, {
      method: 'PUT',
      body: JSON.stringify({ limit_mbps: limitMbps }),
      token,
    }),

  banUser: (token: string, userId: number, ban: boolean) =>
    apiFetch(`/admin/users/${userId}/ban`, {
      method: 'PUT',
      body: JSON.stringify({ ban }),
      token,
    }),

  withdrawals: (token: string) =>
    apiFetch<{ withdrawals: Withdrawal[] }>('/admin/referral/withdrawals', { token }),

  approveWithdrawal: (token: string, id: number, note?: string) =>
    apiFetch(`/admin/referral/withdrawals/${id}/approve`, {
      method: 'PUT',
      body: JSON.stringify({ note }),
      token,
    }),

  rejectWithdrawal: (token: string, id: number, note?: string) =>
    apiFetch(`/admin/referral/withdrawals/${id}/reject`, {
      method: 'PUT',
      body: JSON.stringify({ note }),
      token,
    }),

  plans: (token: string) =>
    apiFetch<{ plans: SubscriptionPlan[] }>('/admin/plans', { token }),

  updatePlanPrice: (token: string, planKey: string, priceRub: number) =>
    apiFetch(`/admin/plans/${planKey}/price`, {
      method: 'PUT',
      body: JSON.stringify({ price_rub: priceRub }),
      token,
    }),

  releases: (token: string) =>
    apiFetch<{ releases: AppRelease[] }>('/admin/releases', { token }),

  createRelease: (token: string, data: {
    platform: string; version: string; download_url: string;
    file_name?: string; file_size_bytes?: number; sha256_checksum?: string;
    changelog?: string; min_os_version?: string; set_latest?: boolean;
  }) =>
    apiFetch<AppRelease>('/admin/releases', { method: 'POST', body: JSON.stringify(data), token }),

  setReleaseLatest: (token: string, id: number) =>
    apiFetch(`/admin/releases/${id}/latest`, { method: 'PUT', token }),

  deleteRelease: (token: string, id: number) =>
    apiFetch(`/admin/releases/${id}`, { method: 'DELETE', token }),
};

// ── App versions (public) ─────────────────────────────────────────────────────

export interface AppRelease {
  id: number;
  platform: string;
  version: string;
  is_latest: boolean;
  download_url: string;
  file_name: string | null;
  file_size_bytes: number | null;
  sha256_checksum: string | null;
  changelog: string | null;
  min_os_version: string | null;
  released_at: string;
}

export const versions = {
  latest: (platform: string) =>
    apiFetch<AppRelease & { download_url: string }>(`/api/version/${platform}`),

  all: () =>
    apiFetch<{ releases: AppRelease[] }>('/api/versions'),
};
