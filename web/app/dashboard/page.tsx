'use client';

import { useEffect, useState, useCallback } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useRouter } from 'next/navigation';
import {
  Shield, CreditCard, Zap, Clock, LogOut, RefreshCw,
  Copy, Check, Star, Gift, Send, History, Tag, X, QrCode
} from 'lucide-react';
import { useAuthStore } from '@/store/auth';
import { subscription, payments, referral, promos, SubscriptionPlan, ReferralStats, CreatePaymentResponse, PaymentStatusResponse } from '@/lib/api';
import { formatDate, formatRub, formatSpeed, getDaysLeft, getStatusLabel, isExpiringSoon } from '@/lib/utils';
import QRCodeCanvas from './QRCodeCanvas';
import Link from 'next/link';

export default function DashboardPage() {
  const router = useRouter();
  const { token, user, logout, refreshUser } = useAuthStore();
  const [plans, setPlans] = useState<SubscriptionPlan[]>([]);
  const [refStats, setRefStats] = useState<ReferralStats | null>(null);
  const [activeTab, setActiveTab] = useState<'home' | 'plans' | 'referral' | 'history'>('home');

  // Payment modal state
  const [payModal, setPayModal] = useState(false);
  const [payAmount, setPayAmount] = useState('');
  const [payPurpose, setPayPurpose] = useState<'balance' | 'subscription'>('balance');
  const [payPlanId, setPayPlanId] = useState('');
  const [paymentData, setPaymentData] = useState<CreatePaymentResponse | null>(null);
  const [payStatus, setPayStatus] = useState<PaymentStatusResponse | null>(null);
  const [polling, setPolling] = useState(false);
  const [payLoading, setPayLoading] = useState(false);

  // Promo modal
  const [promoModal, setPromoModal] = useState(false);
  const [promoCode, setPromoCode] = useState('');
  const [promoMsg, setPromoMsg] = useState('');
  const [promoLoading, setPromoLoading] = useState(false);

  // Referral withdrawal modal
  const [withdrawModal, setWithdrawModal] = useState(false);
  const [withdrawAmount, setWithdrawAmount] = useState('');
  const [withdrawCard, setWithdrawCard] = useState('');
  const [withdrawBank, setWithdrawBank] = useState('');
  const [withdrawMsg, setWithdrawMsg] = useState('');

  const [copied, setCopied] = useState(false);

  // Redirect if not authenticated
  useEffect(() => {
    if (!token) {
      router.push('/auth/login');
    }
  }, [token, router]);

  // Load initial data
  useEffect(() => {
    if (!token) return;
    subscription.plans().then(r => setPlans(r.plans)).catch(() => {});
    referral.stats(token).then(setRefStats).catch(() => {});
    refreshUser();
  }, [token]);

  // Poll payment status
  useEffect(() => {
    if (!paymentData || !token) return;
    if (payStatus?.status === 'paid') return;

    setPolling(true);
    const interval = setInterval(async () => {
      try {
        const status = await payments.status(token, paymentData.payment_id);
        setPayStatus(status);
        if (status.status === 'paid' || status.status === 'expired' || status.status === 'failed') {
          clearInterval(interval);
          setPolling(false);
          if (status.status === 'paid') {
            await refreshUser();
            referral.stats(token).then(setRefStats).catch(() => {});
          }
        }
      } catch {}
    }, 2500);

    return () => clearInterval(interval);
  }, [paymentData, token, payStatus?.status]);

  const handleCreatePayment = async () => {
    if (!token) return;
    const amount = parseFloat(payAmount);
    if (isNaN(amount) || amount < 10) return;
    setPayLoading(true);
    try {
      const data = await payments.createSbp(
        token, amount, payPurpose,
        payPurpose === 'subscription' ? payPlanId : undefined
      );
      setPaymentData(data);
      setPayStatus(null);
    } catch (err: any) {
      alert(err.message);
    } finally {
      setPayLoading(false);
    }
  };

  const handleApplyPromo = async () => {
    if (!token || !promoCode) return;
    setPromoLoading(true);
    try {
      const res = await promos.apply(token, promoCode);
      setPromoMsg(res.message);
      await refreshUser();
    } catch (err: any) {
      setPromoMsg(err.message || 'Ошибка');
    } finally {
      setPromoLoading(false);
    }
  };

  const handleWithdraw = async () => {
    if (!token) return;
    const amount = parseFloat(withdrawAmount);
    try {
      const res = await referral.withdraw(token, amount, withdrawCard, withdrawBank || undefined);
      setWithdrawMsg(res.message);
      referral.stats(token).then(setRefStats).catch(() => {});
    } catch (err: any) {
      setWithdrawMsg(err.message || 'Ошибка');
    }
  };

  const copyReferral = () => {
    const code = user?.referral_code || '';
    const url = `${window.location.origin}/auth/register?ref=${code}`;
    navigator.clipboard.writeText(url);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  if (!token || !user) return null;

  const daysLeft = getDaysLeft(user.sub_expires_at);
  const expiringSoon = isExpiringSoon(user.sub_expires_at);

  return (
    <div className="min-h-screen" style={{ background: 'var(--background)' }}>
      {/* Header */}
      <header className="glass border-b sticky top-0 z-40" style={{ borderColor: 'var(--border)' }}>
        <div className="max-w-5xl mx-auto px-4 h-16 flex items-center justify-between">
          <Link href="/" className="flex items-center gap-2">
            <div className="w-8 h-8 rounded-lg flex items-center justify-center"
              style={{ background: 'linear-gradient(135deg, #3b82f6, #2563eb)' }}>
              <Shield className="w-4 h-4 text-white" />
            </div>
            <span className="font-bold gradient-text">Lowkey VPN</span>
          </Link>
          <div className="flex items-center gap-3">
            <span className="text-sm" style={{ color: 'var(--muted-foreground)' }}>{user.login}</span>
            {user.role === 'admin' && (
              <Link href="/admin">
                <span className="badge badge-active text-xs">Admin</span>
              </Link>
            )}
            <button onClick={logout} className="btn btn-secondary text-sm py-1.5 px-3">
              <LogOut className="w-4 h-4" />
            </button>
          </div>
        </div>
      </header>

      {/* Tabs */}
      <div className="border-b" style={{ borderColor: 'var(--border)' }}>
        <div className="max-w-5xl mx-auto px-4 flex gap-1 pt-2">
          {[
            { id: 'home', label: 'Главная', icon: Shield },
            { id: 'plans', label: 'Тарифы', icon: Zap },
            { id: 'referral', label: 'Рефералы', icon: Star },
            { id: 'history', label: 'История', icon: History },
          ].map(tab => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id as any)}
              className={`flex items-center gap-2 px-4 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors ${
                activeTab === tab.id
                  ? 'border-blue-400 text-white'
                  : 'border-transparent hover:text-white'
              }`}
              style={{ color: activeTab === tab.id ? 'var(--foreground)' : 'var(--muted-foreground)' }}
            >
              <tab.icon className="w-4 h-4" />
              {tab.label}
            </button>
          ))}
        </div>
      </div>

      <main className="max-w-5xl mx-auto px-4 py-8">
        {/* Home Tab */}
        {activeTab === 'home' && (
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            className="space-y-6"
          >
            {/* Expiry warning */}
            {expiringSoon && (
              <motion.div
                initial={{ opacity: 0, scale: 0.95 }}
                animate={{ opacity: 1, scale: 1 }}
                className="rounded-2xl p-4 flex items-center gap-3"
                style={{ background: 'rgba(255,165,0,0.1)', border: '1px solid rgba(255,165,0,0.3)' }}
              >
                <Clock className="w-5 h-5 flex-shrink-0" style={{ color: '#ffa500' }} />
                <div>
                  <div className="font-semibold" style={{ color: '#ffa500' }}>Подписка скоро закончится</div>
                  <div className="text-sm" style={{ color: 'var(--muted-foreground)' }}>
                    Осталось {daysLeft} {daysLeft === 1 ? 'день' : daysLeft < 5 ? 'дня' : 'дней'}. Пополните баланс и продлите подписку.
                  </div>
                </div>
              </motion.div>
            )}

            {/* Stats grid */}
            <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
              {/* Balance */}
              <div className="glass rounded-2xl p-6 card-hover">
                <div className="flex items-center gap-2 mb-4">
                  <CreditCard className="w-5 h-5" style={{ color: '#60a5fa' }} />
                  <span className="text-sm font-medium" style={{ color: 'var(--muted-foreground)' }}>Баланс</span>
                </div>
                <div className="text-3xl font-bold">{formatRub(user.balance)}</div>
                <button
                  onClick={() => { setPayModal(true); setPayPurpose('balance'); }}
                  className="btn btn-primary mt-4 w-full text-sm"
                >
                  Пополнить через СБП
                </button>
              </div>

              {/* Subscription */}
              <div className="glass rounded-2xl p-6 card-hover">
                <div className="flex items-center gap-2 mb-4">
                  <Shield className="w-5 h-5" style={{ color: user.sub_status === 'active' ? '#60a5fa' : '#f87171' }} />
                  <span className="text-sm font-medium" style={{ color: 'var(--muted-foreground)' }}>Подписка</span>
                </div>
                <div className="flex items-center gap-2 mb-2">
                  <span className={`badge ${user.sub_status === 'active' ? 'badge-active' : 'badge-expired'}`}>
                    {getStatusLabel(user.sub_status)}
                  </span>
                </div>
                {user.sub_expires_at && (
                  <div className="text-sm" style={{ color: 'var(--muted-foreground)' }}>
                    До {formatDate(user.sub_expires_at)}
                  </div>
                )}
                <div className="text-sm mt-1" style={{ color: 'var(--muted-foreground)' }}>
                  {formatSpeed(user.sub_speed_mbps)}
                </div>
              </div>

              {/* Referral balance */}
              <div className="glass rounded-2xl p-6 card-hover">
                <div className="flex items-center gap-2 mb-4">
                  <Star className="w-5 h-5" style={{ color: '#ffa500' }} />
                  <span className="text-sm font-medium" style={{ color: 'var(--muted-foreground)' }}>Реферальный баланс</span>
                </div>
                <div className="text-3xl font-bold">{formatRub(user.referral_balance)}</div>
                <button
                  onClick={() => setWithdrawModal(true)}
                  disabled={user.referral_balance < 100}
                  className="btn btn-secondary mt-4 w-full text-sm"
                >
                  Вывести на карту
                </button>
              </div>
            </div>

            {/* Promo code */}
            <div className="glass rounded-2xl p-6">
              <div className="flex items-center gap-2 mb-4">
                <Tag className="w-5 h-5" style={{ color: '#60a5fa' }} />
                <h3 className="font-semibold">Промокод</h3>
              </div>
              <div className="flex gap-3">
                <input
                  type="text"
                  value={promoCode}
                  onChange={e => setPromoCode(e.target.value.toUpperCase())}
                  placeholder="Введите промокод"
                  className="flex-1"
                />
                <button onClick={handleApplyPromo} disabled={promoLoading || !promoCode}
                  className="btn btn-primary">
                  {promoLoading ? <div className="w-4 h-4 border-2 border-black/30 border-t-black rounded-full animate-spin" /> : 'Применить'}
                </button>
              </div>
              {promoMsg && (
                <motion.p initial={{ opacity: 0 }} animate={{ opacity: 1 }}
                  className="mt-3 text-sm" style={{ color: '#60a5fa' }}>
                  {promoMsg}
                </motion.p>
              )}
            </div>
          </motion.div>
        )}

        {/* Plans Tab */}
        {activeTab === 'plans' && (
          <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }}>
            {user.first_purchase_done === false && user.referral_balance !== undefined && (
              <div className="rounded-2xl p-4 mb-6 flex items-center gap-3"
                style={{ background: 'rgba(59,130,246,0.08)', border: '1px solid rgba(59,130,246,0.25)' }}>
                <Gift className="w-5 h-5" style={{ color: '#60a5fa' }} />
                <div className="text-sm">
                  <span className="font-semibold" style={{ color: '#60a5fa' }}>Скидка 50%</span>
                  <span style={{ color: 'var(--muted-foreground)' }}> на первую подписку (реферальная программа)</span>
                </div>
              </div>
            )}

            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
              {plans.map((plan, i) => {
                const planKey = plan.plan_key || plan.id?.toString() || '';
                const isBundle = plan.is_bundle;
                return (
                  <div key={i} className={`glass rounded-2xl p-6 card-hover ${isBundle ? 'ring-1' : ''}`}
                    style={isBundle ? { borderColor: 'rgba(255,165,0,0.3)' } : {}}>
                    {plan.discount_pct ? (
                      <div className="badge badge-pending mb-3">−{plan.discount_pct}%</div>
                    ) : null}
                    <h3 className="font-semibold text-lg mb-1">{plan.name}</h3>
                    <div className="flex items-baseline gap-1 mb-2">
                      <span className="text-3xl font-bold gradient-text">{formatRub(plan.price_rub)}</span>
                      <span className="text-sm" style={{ color: 'var(--muted-foreground)' }}>/ {plan.duration_days} дней</span>
                    </div>
                    <p className="text-sm mb-4" style={{ color: 'var(--muted-foreground)' }}>{formatSpeed(plan.speed_mbps)}</p>
                    <div className="flex flex-col gap-2">
                      {user.balance >= plan.price_rub ? (
                        <button
                          className="btn btn-primary w-full text-sm"
                          onClick={async () => {
                            if (!token) return;
                            try {
                              const res = await subscription.buy(token, planKey || plan.name.toLowerCase());
                              alert(`Подписка оформлена до ${formatDate(res.expires_at)}`);
                              await refreshUser();
                            } catch (e: any) { alert(e.message); }
                          }}
                        >
                          Купить за баланс
                        </button>
                      ) : null}
                      <button
                        className="btn btn-secondary w-full text-sm"
                        onClick={() => {
                          setPayPurpose('subscription');
                          setPayPlanId(planKey || '');
                          setPayAmount(plan.price_rub.toString());
                          setPayModal(true);
                        }}
                      >
                        <QrCode className="w-4 h-4" />
                        Оплатить по СБП
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          </motion.div>
        )}

        {/* Referral Tab */}
        {activeTab === 'referral' && refStats && (
          <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} className="space-y-6">
            <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
              <div className="glass rounded-2xl p-6 text-center">
                <div className="text-4xl font-bold gradient-text mb-2">{refStats.referral_count}</div>
                <div className="text-sm" style={{ color: 'var(--muted-foreground)' }}>Приглашённых</div>
              </div>
              <div className="glass rounded-2xl p-6 text-center">
                <div className="text-4xl font-bold gradient-text mb-2">{formatRub(refStats.total_earned)}</div>
                <div className="text-sm" style={{ color: 'var(--muted-foreground)' }}>Всего заработано</div>
              </div>
              <div className="glass rounded-2xl p-6 text-center">
                <div className="text-4xl font-bold gradient-text mb-2">{formatRub(refStats.referral_balance)}</div>
                <div className="text-sm" style={{ color: 'var(--muted-foreground)' }}>Доступно к выводу</div>
              </div>
            </div>

            <div className="glass rounded-2xl p-6">
              <h3 className="font-semibold mb-4">Ваша реферальная ссылка</h3>
              <div className="flex gap-3">
                <input
                  readOnly
                  value={`${typeof window !== 'undefined' ? window.location.origin : ''}/auth/register?ref=${refStats.referral_code || ''}`}
                  className="flex-1 text-sm"
                  style={{ color: 'var(--muted-foreground)' }}
                />
                <button onClick={copyReferral} className="btn btn-primary text-sm">
                  {copied ? <Check className="w-4 h-4" /> : <Copy className="w-4 h-4" />}
                  {copied ? 'Скопировано' : 'Копировать'}
                </button>
              </div>
              <div className="mt-4 p-4 rounded-xl text-sm space-y-2"
                style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.05)' }}>
                <div className="flex items-center gap-2">
                  <Check className="w-4 h-4 flex-shrink-0" style={{ color: '#60a5fa' }} />
                  <span style={{ color: 'var(--muted-foreground)' }}>Вы получаете <strong className="text-white">25%</strong> с каждого платежа реферала</span>
                </div>
                <div className="flex items-center gap-2">
                  <Check className="w-4 h-4 flex-shrink-0" style={{ color: '#60a5fa' }} />
                  <span style={{ color: 'var(--muted-foreground)' }}>Ваш друг получает <strong className="text-white">скидку 50%</strong> на первую подписку</span>
                </div>
                <div className="flex items-center gap-2">
                  <Check className="w-4 h-4 flex-shrink-0" style={{ color: '#60a5fa' }} />
                  <span style={{ color: 'var(--muted-foreground)' }}>Минимальная сумма вывода: <strong className="text-white">100 ₽</strong></span>
                </div>
              </div>
            </div>

            {refStats.referral_balance >= 100 && (
              <button onClick={() => setWithdrawModal(true)}
                className="btn btn-primary w-full py-3 glow-blue">
                <Send className="w-4 h-4" />
                Вывести {formatRub(refStats.referral_balance)} на карту
              </button>
            )}
          </motion.div>
        )}

        {/* History Tab */}
        {activeTab === 'history' && (
          <HistoryTab token={token} />
        )}
      </main>

      {/* Payment Modal */}
      <AnimatePresence>
        {payModal && (
          <PaymentModal
            payPurpose={payPurpose}
            setPayPurpose={setPayPurpose}
            payAmount={payAmount}
            setPayAmount={setPayAmount}
            payPlanId={payPlanId}
            setPayPlanId={setPayPlanId}
            plans={plans}
            paymentData={paymentData}
            payStatus={payStatus}
            polling={polling}
            payLoading={payLoading}
            onClose={() => { setPayModal(false); setPaymentData(null); setPayStatus(null); }}
            onCreate={handleCreatePayment}
            hasDiscount={!user.first_purchase_done && !!user.referral_code}
          />
        )}
      </AnimatePresence>

      {/* Promo Modal */}
      <AnimatePresence>
        {promoModal && (
          <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
            <motion.div initial={{ opacity: 0, scale: 0.9 }} animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.9 }} className="glass rounded-2xl p-8 w-full max-w-md m-4">
              <h3 className="text-xl font-bold mb-4">Промокод</h3>
              <input type="text" value={promoCode} onChange={e => setPromoCode(e.target.value.toUpperCase())}
                placeholder="XXXXXXXX" className="w-full mb-4" />
              {promoMsg && <p className="text-sm mb-4" style={{ color: '#60a5fa' }}>{promoMsg}</p>}
              <div className="flex gap-3">
                <button onClick={() => setPromoModal(false)} className="btn btn-secondary flex-1">Закрыть</button>
                <button onClick={handleApplyPromo} disabled={promoLoading || !promoCode}
                  className="btn btn-primary flex-1">Применить</button>
              </div>
            </motion.div>
          </div>
        )}
      </AnimatePresence>

      {/* Withdrawal Modal */}
      <AnimatePresence>
        {withdrawModal && (
          <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
            <motion.div initial={{ opacity: 0, scale: 0.9 }} animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.9 }} className="glass rounded-2xl p-8 w-full max-w-md m-4">
              <h3 className="text-xl font-bold mb-6">Вывод реферальных средств</h3>
              {withdrawMsg ? (
                <div>
                  <p className="text-sm mb-6" style={{ color: '#60a5fa' }}>{withdrawMsg}</p>
                  <button onClick={() => { setWithdrawModal(false); setWithdrawMsg(''); }}
                    className="btn btn-primary w-full">Закрыть</button>
                </div>
              ) : (
                <div className="space-y-4">
                  <div>
                    <label className="block text-sm mb-1.5" style={{ color: 'var(--muted-foreground)' }}>Сумма (₽)</label>
                    <input type="number" min="100" value={withdrawAmount}
                      onChange={e => setWithdrawAmount(e.target.value)} className="w-full"
                      placeholder="Минимум 100 ₽" />
                  </div>
                  <div>
                    <label className="block text-sm mb-1.5" style={{ color: 'var(--muted-foreground)' }}>Номер карты</label>
                    <input type="text" value={withdrawCard}
                      onChange={e => setWithdrawCard(e.target.value)} className="w-full"
                      placeholder="1234 5678 9012 3456" />
                  </div>
                  <div>
                    <label className="block text-sm mb-1.5" style={{ color: 'var(--muted-foreground)' }}>Банк (необязательно)</label>
                    <input type="text" value={withdrawBank}
                      onChange={e => setWithdrawBank(e.target.value)} className="w-full"
                      placeholder="Сбербанк, Тинькофф..." />
                  </div>
                  <div className="flex gap-3 pt-2">
                    <button onClick={() => setWithdrawModal(false)} className="btn btn-secondary flex-1">Отмена</button>
                    <button onClick={handleWithdraw}
                      disabled={!withdrawAmount || !withdrawCard || parseFloat(withdrawAmount) < 100}
                      className="btn btn-primary flex-1">Подать заявку</button>
                  </div>
                </div>
              )}
            </motion.div>
          </div>
        )}
      </AnimatePresence>
    </div>
  );
}

// ── Payment Modal Component ───────────────────────────────────────────────────

function PaymentModal({
  payPurpose, setPayPurpose, payAmount, setPayAmount,
  payPlanId, setPayPlanId, plans, paymentData, payStatus,
  polling, payLoading, onClose, onCreate, hasDiscount
}: {
  payPurpose: 'balance' | 'subscription';
  setPayPurpose: (v: 'balance' | 'subscription') => void;
  payAmount: string;
  setPayAmount: (v: string) => void;
  payPlanId: string;
  setPayPlanId: (v: string) => void;
  plans: SubscriptionPlan[];
  paymentData: CreatePaymentResponse | null;
  payStatus: PaymentStatusResponse | null;
  polling: boolean;
  payLoading: boolean;
  onClose: () => void;
  onCreate: () => void;
  hasDiscount: boolean;
}) {
  const isPaid = payStatus?.status === 'paid';

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm px-4">
      <motion.div
        initial={{ opacity: 0, scale: 0.9, y: 20 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        exit={{ opacity: 0, scale: 0.9, y: 20 }}
        className="glass rounded-3xl p-8 w-full max-w-md relative"
      >
        <button onClick={onClose}
          className="absolute top-4 right-4 p-2 rounded-lg hover:bg-white/5 transition-colors">
          <X className="w-5 h-5" style={{ color: 'var(--muted-foreground)' }} />
        </button>

        <h3 className="text-xl font-bold mb-6">Оплата через СБП</h3>

        {!paymentData ? (
          <div className="space-y-4">
            {hasDiscount && (
              <div className="rounded-xl p-3 text-sm flex items-center gap-2"
                style={{ background: 'rgba(59,130,246,0.08)', border: '1px solid rgba(59,130,246,0.2)', color: '#60a5fa' }}>
                <Gift className="w-4 h-4" />
                Скидка 50% на первую подписку будет применена автоматически
              </div>
            )}

            <div className="flex gap-2">
              {(['balance', 'subscription'] as const).map(p => (
                <button key={p} onClick={() => setPayPurpose(p)}
                  className={`flex-1 py-2.5 rounded-xl text-sm font-medium transition-colors ${
                    payPurpose === p ? 'bg-blue-400/20 text-white border border-blue-400/50' : 'btn btn-secondary'
                  }`}>
                  {p === 'balance' ? 'Пополнить баланс' : 'Купить подписку'}
                </button>
              ))}
            </div>

            {payPurpose === 'subscription' && (
              <select value={payPlanId} onChange={e => {
                setPayPlanId(e.target.value);
                const plan = plans.find(p => (p.plan_key || '') === e.target.value);
                if (plan) setPayAmount(plan.price_rub.toString());
              }} className="w-full">
                <option value="">Выберите тариф</option>
                {plans.map((p, i) => (
                  <option key={i} value={p.plan_key || ''}>
                    {p.name} — {formatRub(p.price_rub)}
                  </option>
                ))}
              </select>
            )}

            <div>
              <label className="block text-sm mb-1.5" style={{ color: 'var(--muted-foreground)' }}>
                Сумма (₽) {hasDiscount && payPurpose === 'subscription' ? '— будет применена скидка 50%' : ''}
              </label>
              <input
                type="number"
                value={payAmount}
                onChange={e => setPayAmount(e.target.value)}
                min="10"
                max="100000"
                placeholder="Минимум 10 ₽"
                className="w-full"
                readOnly={payPurpose === 'subscription' && !!payPlanId}
              />
            </div>

            <button
              onClick={onCreate}
              disabled={payLoading || !payAmount || parseFloat(payAmount) < 10}
              className="btn btn-primary w-full py-3 glow-blue"
            >
              {payLoading ? (
                <span className="flex items-center gap-2">
                  <div className="w-4 h-4 border-2 border-black/30 border-t-black rounded-full animate-spin" />
                  Создание...
                </span>
              ) : (
                <span className="flex items-center gap-2">
                  <QrCode className="w-4 h-4" />
                  Сгенерировать QR-код
                </span>
              )}
            </button>
          </div>
        ) : (
          <div className="text-center">
            {isPaid ? (
              <motion.div
                initial={{ opacity: 0, scale: 0.8 }}
                animate={{ opacity: 1, scale: 1 }}
                className="space-y-4"
              >
                <div className="w-20 h-20 rounded-full mx-auto flex items-center justify-center glow-blue"
                  style={{ background: 'rgba(59,130,246,0.2)' }}>
                  <Check className="w-10 h-10" style={{ color: '#60a5fa' }} />
                </div>
                <h4 className="text-xl font-bold" style={{ color: '#60a5fa' }}>Платёж получен!</h4>
                <p style={{ color: 'var(--muted-foreground)' }}>
                  {payPurpose === 'subscription' && payStatus?.sub_expires_at
                    ? `Подписка активна до ${formatDate(payStatus.sub_expires_at)}`
                    : `Баланс пополнен на ${formatRub(paymentData.amount)}`}
                </p>
                <button onClick={onClose} className="btn btn-primary w-full">Закрыть</button>
              </motion.div>
            ) : (
              <div className="space-y-4">
                <p className="text-sm mb-4" style={{ color: 'var(--muted-foreground)' }}>
                  Отсканируйте QR-код в приложении вашего банка для оплаты {formatRub(paymentData.amount)}
                </p>

                {/* QR Code */}
                <div className="p-4 rounded-2xl mx-auto w-fit"
                  style={{ background: 'white' }}>
                  <QRCodeCanvas value={paymentData.qr_payload} size={200} />
                </div>

                {/* Polling indicator */}
                <div className="flex items-center justify-center gap-2 text-sm"
                  style={{ color: 'var(--muted-foreground)' }}>
                  {polling && (
                    <div className="w-3 h-3 border-2 border-blue-400/30 border-t-blue-400 rounded-full animate-spin" />
                  )}
                  <span>
                    {payStatus?.status === 'expired'
                      ? 'QR-код устарел. Создайте новый.'
                      : polling
                        ? 'Ожидание оплаты...'
                        : 'Проверяем статус...'}
                  </span>
                </div>

                <div className="p-3 rounded-xl text-xs break-all text-left"
                  style={{ background: 'rgba(255,255,255,0.03)', color: 'var(--muted-foreground)' }}>
                  {paymentData.qr_payload}
                </div>

                <button onClick={onClose} className="btn btn-secondary w-full">Закрыть</button>
              </div>
            )}
          </div>
        )}
      </motion.div>
    </div>
  );
}

// ── History Tab ───────────────────────────────────────────────────────────────

function HistoryTab({ token }: { token: string }) {
  const [payHistory, setPayHistory] = useState<any[]>([]);
  const [withdrawHistory, setWithdrawHistory] = useState<any[]>([]);

  useEffect(() => {
    payments.history(token).then(r => setPayHistory(r.payments)).catch(() => {});
    referral.withdrawals(token).then(r => setWithdrawHistory(r.withdrawals)).catch(() => {});
  }, [token]);

  return (
    <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} className="space-y-6">
      <div className="glass rounded-2xl overflow-hidden">
        <div className="px-6 py-4 border-b" style={{ borderColor: 'var(--border)' }}>
          <h3 className="font-semibold">История платежей</h3>
        </div>
        {payHistory.length === 0 ? (
          <p className="px-6 py-8 text-center text-sm" style={{ color: 'var(--muted-foreground)' }}>
            Нет платежей
          </p>
        ) : (
          <div className="divide-y" style={{ borderColor: 'var(--border)' }}>
            {payHistory.map(p => (
              <div key={p.id} className="px-6 py-4 flex items-center justify-between">
                <div>
                  <div className="font-medium">{p.purpose === 'balance' ? 'Пополнение баланса' : 'Подписка'}</div>
                  <div className="text-sm" style={{ color: 'var(--muted-foreground)' }}>
                    {new Date(p.created_at).toLocaleDateString('ru-RU')}
                  </div>
                </div>
                <div className="text-right">
                  <div className="font-semibold">{formatRub(p.amount)}</div>
                  <span className={`badge text-xs ${p.status === 'paid' ? 'badge-active' : p.status === 'pending' ? 'badge-pending' : 'badge-expired'}`}>
                    {getStatusLabel(p.status)}
                  </span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="glass rounded-2xl overflow-hidden">
        <div className="px-6 py-4 border-b" style={{ borderColor: 'var(--border)' }}>
          <h3 className="font-semibold">Заявки на вывод</h3>
        </div>
        {withdrawHistory.length === 0 ? (
          <p className="px-6 py-8 text-center text-sm" style={{ color: 'var(--muted-foreground)' }}>
            Нет заявок
          </p>
        ) : (
          <div className="divide-y" style={{ borderColor: 'var(--border)' }}>
            {withdrawHistory.map(w => (
              <div key={w.id} className="px-6 py-4 flex items-center justify-between">
                <div>
                  <div className="font-medium">Вывод на карту ****{w.card_number.slice(-4)}</div>
                  <div className="text-sm" style={{ color: 'var(--muted-foreground)' }}>
                    {new Date(w.requested_at).toLocaleDateString('ru-RU')}
                    {w.admin_note && ` · ${w.admin_note}`}
                  </div>
                </div>
                <div className="text-right">
                  <div className="font-semibold">{formatRub(w.amount)}</div>
                  <span className={`badge text-xs ${w.status === 'completed' ? 'badge-active' : w.status === 'pending' ? 'badge-pending' : 'badge-expired'}`}>
                    {getStatusLabel(w.status)}
                  </span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </motion.div>
  );
}
