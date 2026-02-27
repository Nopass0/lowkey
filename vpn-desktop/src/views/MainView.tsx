import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { motion, AnimatePresence } from 'framer-motion';
import {
  Shield, Wifi, WifiOff, LogOut, CreditCard,
  RefreshCw, X, QrCode, Clock, Star
} from 'lucide-react';
import { useStore } from '../store';
import QRCodeView from '../components/QRCodeView';

export default function MainView() {
  const { token, user, setUser, connected, setConnected, logout, apiUrl } = useStore();
  const [toggling, setToggling] = useState(false);
  const [activeTab, setActiveTab] = useState<'home' | 'plans' | 'referral'>('home');
  const [plans, setPlans] = useState<any[]>([]);
  const [refStats, setRefStats] = useState<any>(null);
  const [payModal, setPayModal] = useState(false);
  const [payAmount, setPayAmount] = useState('');
  const [payPurpose, setPayPurpose] = useState<'balance' | 'subscription'>('balance');
  const [payPlanId, setPayPlanId] = useState('');
  const [payData, setPayData] = useState<any>(null);
  const [payStatus, setPayStatus] = useState<any>(null);
  const [payLoading, setPayLoading] = useState(false);
  const pollRef = useRef<number | null>(null);

  const refreshUser = async () => {
    if (!token) return;
    try {
      const u = await invoke<any>('get_user_info', { apiUrl, token });
      setUser(u);
    } catch {}
  };

  useEffect(() => {
    invoke<any>('get_plans', { apiUrl }).then(r => setPlans(r.plans || [])).catch(() => {});
    invoke<any>('get_referral_stats', { apiUrl, token }).then(setRefStats).catch(() => {});

    // Check initial VPN status
    invoke<any>('vpn_status').then(s => {
      setConnected(s.connected, s.vpn_ip);
    }).catch(() => {});
  }, []);

  // Poll payment
  useEffect(() => {
    if (!payData || !token) return;
    if (payStatus?.status === 'paid') return;

    pollRef.current = window.setInterval(async () => {
      try {
        const status = await invoke<any>('poll_payment_status', {
          apiUrl, token, paymentId: payData.payment_id
        });
        setPayStatus(status);
        if (status.status === 'paid' || status.status === 'expired') {
          clearInterval(pollRef.current!);
          if (status.status === 'paid') {
            await refreshUser();
          }
        }
      } catch {}
    }, 2500);

    return () => { if (pollRef.current) clearInterval(pollRef.current); };
  }, [payData, payStatus?.status]);

  const handleToggle = async () => {
    if (!token) return;
    setToggling(true);
    try {
      const res = await invoke<any>('toggle_vpn', { token, apiUrl });
      setConnected(res.connected, res.vpn_ip);
    } catch (err: any) {
      alert(typeof err === 'string' ? err : 'Ошибка подключения');
    } finally {
      setToggling(false);
    }
  };

  const handleCreatePayment = async () => {
    if (!token) return;
    setPayLoading(true);
    try {
      const data = await invoke<any>('create_sbp_payment', {
        apiUrl, token,
        amount: parseFloat(payAmount),
        purpose: payPurpose,
        planId: payPurpose === 'subscription' ? payPlanId : null,
      });
      setPayData(data);
      setPayStatus(null);
    } catch (err: any) {
      alert(typeof err === 'string' ? err : 'Ошибка создания платежа');
    } finally {
      setPayLoading(false);
    }
  };

  const daysLeft = user?.sub_expires_at
    ? Math.max(0, Math.ceil((new Date(user.sub_expires_at).getTime() - Date.now()) / 86400000))
    : 0;

  if (!user) return null;

  return (
    <div className="h-screen flex flex-col" style={{ background: 'var(--bg)' }}>
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b" style={{ borderColor: 'var(--border)' }}>
        <div className="flex items-center gap-2">
          <div className="w-6 h-6 rounded flex items-center justify-center"
            style={{ background: 'linear-gradient(135deg, #3b82f6, #2563eb)' }}>
            <Shield className="w-3 h-3 text-white" />
          </div>
          <span className="text-sm font-bold gradient-text">Lowkey VPN</span>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-xs" style={{ color: 'var(--muted)' }}>{user.login}</span>
          <button onClick={logout} className="p-1.5 rounded hover:bg-white/5 transition-colors">
            <LogOut className="w-3.5 h-3.5" style={{ color: 'var(--muted)' }} />
          </button>
        </div>
      </div>

      {/* Tabs */}
      <div className="flex border-b px-4" style={{ borderColor: 'var(--border)' }}>
        {[
          { id: 'home', label: 'Главная' },
          { id: 'plans', label: 'Тарифы' },
          { id: 'referral', label: 'Рефералы' },
        ].map(t => (
          <button key={t.id} onClick={() => setActiveTab(t.id as any)}
            className={`px-3 py-2 text-xs font-medium border-b-2 -mb-px transition-colors ${
              activeTab === t.id ? 'border-blue-400 text-white' : 'border-transparent'
            }`}
            style={{ color: activeTab === t.id ? 'var(--text)' : 'var(--muted)' }}>
            {t.label}
          </button>
        ))}
      </div>

      <div className="flex-1 overflow-auto p-4">
        {/* HOME TAB */}
        {activeTab === 'home' && (
          <div className="space-y-4">
            {/* VPN Toggle */}
            <div className="glass rounded-2xl p-6 text-center">
              <div className="relative inline-block mb-4">
                <motion.button
                  onClick={handleToggle}
                  disabled={toggling}
                  whileHover={{ scale: 1.02 }}
                  whileTap={{ scale: 0.98 }}
                  className="w-28 h-28 rounded-full flex items-center justify-center transition-all duration-500 relative"
                  style={{
                    background: connected
                      ? 'radial-gradient(circle, rgba(59,130,246,0.3), rgba(59,130,246,0.1))'
                      : 'radial-gradient(circle, rgba(255,68,68,0.2), rgba(255,68,68,0.05))',
                    border: `3px solid ${connected ? '#3b82f6' : 'rgba(255,68,68,0.5)'}`,
                    boxShadow: connected
                      ? '0 0 40px rgba(59,130,246,0.3), inset 0 0 20px rgba(59,130,246,0.05)'
                      : '0 0 20px rgba(255,68,68,0.2)',
                  }}
                >
                  {toggling ? (
                    <div className="w-8 h-8 border-3 border-white/20 border-t-white rounded-full animate-spin" />
                  ) : connected ? (
                    <Wifi className="w-10 h-10" style={{ color: '#3b82f6' }} />
                  ) : (
                    <WifiOff className="w-10 h-10" style={{ color: '#ff4444' }} />
                  )}
                </motion.button>
              </div>

              <div className="text-lg font-bold mb-1" style={{ color: connected ? '#60a5fa' : '#ff4444' }}>
                {toggling ? 'Подключение...' : connected ? 'Подключён' : 'Отключён'}
              </div>
              <div className="text-xs" style={{ color: 'var(--muted)' }}>
                {connected ? 'Трафик защищён' : 'Нажмите для подключения'}
              </div>
            </div>

            {/* Stats */}
            <div className="grid grid-cols-2 gap-3">
              <div className="glass rounded-xl p-3">
                <div className="text-xs mb-1" style={{ color: 'var(--muted)' }}>Баланс</div>
                <div className="font-bold">{user.balance.toFixed(0)} ₽</div>
                <button onClick={() => setPayModal(true)} className="btn btn-primary mt-2 text-xs py-1 w-full">
                  Пополнить СБП
                </button>
              </div>
              <div className="glass rounded-xl p-3">
                <div className="text-xs mb-1" style={{ color: 'var(--muted)' }}>Подписка</div>
                <div className={`font-bold text-sm ${user.sub_status === 'active' ? '' : ''}`}
                  style={{ color: user.sub_status === 'active' ? '#60a5fa' : '#ff4444' }}>
                  {user.sub_status === 'active' ? 'Активна' : 'Неактивна'}
                </div>
                {user.sub_expires_at && (
                  <div className="text-xs mt-1 flex items-center gap-1" style={{ color: 'var(--muted)' }}>
                    <Clock className="w-3 h-3" />
                    {daysLeft}д осталось
                  </div>
                )}
              </div>
            </div>

            {/* Expiry warning */}
            {daysLeft > 0 && daysLeft <= 5 && (
              <div className="rounded-xl p-3 text-xs flex items-center gap-2"
                style={{ background: 'rgba(255,165,0,0.1)', border: '1px solid rgba(255,165,0,0.3)', color: '#ffa500' }}>
                <Clock className="w-4 h-4" />
                Подписка истекает через {daysLeft} {daysLeft === 1 ? 'день' : 'дня'}. Продлите заранее!
              </div>
            )}
          </div>
        )}

        {/* PLANS TAB */}
        {activeTab === 'plans' && (
          <div className="space-y-3">
            {plans.map((p, i) => (
              <div key={i} className="glass rounded-xl p-4">
                <div className="flex justify-between items-start mb-2">
                  <div>
                    <div className="font-semibold text-sm">{p.name}</div>
                    <div className="text-xs" style={{ color: 'var(--muted)' }}>
                      {p.speed_mbps === 0 ? '∞ Мбит/с' : `${p.speed_mbps} Мбит/с`} · {p.duration_days} дней
                    </div>
                  </div>
                  <div className="text-lg font-bold gradient-text">{p.price_rub} ₽</div>
                </div>
                <button
                  onClick={() => {
                    setPayPurpose('subscription');
                    setPayPlanId(p.plan_key || '');
                    setPayAmount(p.price_rub.toString());
                    setPayModal(true);
                  }}
                  className="btn btn-secondary w-full text-xs py-1.5"
                >
                  <QrCode className="w-3.5 h-3.5" />
                  Оплатить через СБП
                </button>
              </div>
            ))}
          </div>
        )}

        {/* REFERRAL TAB */}
        {activeTab === 'referral' && refStats && (
          <div className="space-y-3">
            <div className="glass rounded-xl p-4 text-center">
              <div className="text-xs mb-1" style={{ color: 'var(--muted)' }}>Реферальный баланс</div>
              <div className="text-3xl font-bold gradient-text">{refStats.referral_balance?.toFixed(2)} ₽</div>
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="glass rounded-xl p-3 text-center">
                <div className="text-2xl font-bold gradient-text">{refStats.referral_count}</div>
                <div className="text-xs" style={{ color: 'var(--muted)' }}>Приглашено</div>
              </div>
              <div className="glass rounded-xl p-3 text-center">
                <div className="text-2xl font-bold gradient-text">{refStats.total_earned?.toFixed(0)} ₽</div>
                <div className="text-xs" style={{ color: 'var(--muted)' }}>Заработано</div>
              </div>
            </div>
            <div className="glass rounded-xl p-4">
              <div className="text-xs mb-2 font-medium">Ваш код</div>
              <div className="font-mono text-lg text-center py-2 rounded-lg"
                style={{ background: 'rgba(59,130,246,0.08)', color: '#60a5fa' }}>
                {refStats.referral_code}
              </div>
              <div className="text-xs mt-2" style={{ color: 'var(--muted)' }}>
                Делитесь ссылкой регистрации с другом. Вы получите 25% с каждого его платежа.
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Payment Modal */}
      <AnimatePresence>
        {payModal && (
          <div className="fixed inset-0 z-50 flex items-end justify-center bg-black/70">
            <motion.div
              initial={{ y: '100%' }} animate={{ y: 0 }} exit={{ y: '100%' }}
              transition={{ type: 'spring', stiffness: 300, damping: 30 }}
              className="glass rounded-t-3xl p-6 w-full max-w-sm"
            >
              <div className="flex justify-between items-center mb-4">
                <h3 className="font-bold">Оплата через СБП</h3>
                <button onClick={() => { setPayModal(false); setPayData(null); setPayStatus(null); }}
                  className="p-1">
                  <X className="w-5 h-5" style={{ color: 'var(--muted)' }} />
                </button>
              </div>

              {!payData ? (
                <div className="space-y-3">
                  <div className="flex gap-2">
                    {(['balance', 'subscription'] as const).map(p => (
                      <button key={p} onClick={() => setPayPurpose(p)}
                        className={`flex-1 text-xs py-2 rounded-lg transition-colors ${
                          payPurpose === p ? 'btn-primary' : 'btn-secondary'
                        } btn`}>
                        {p === 'balance' ? 'Пополнить' : 'Подписка'}
                      </button>
                    ))}
                  </div>

                  {payPurpose === 'subscription' && (
                    <select value={payPlanId} onChange={e => {
                      setPayPlanId(e.target.value);
                      const plan = plans.find(p => p.plan_key === e.target.value);
                      if (plan) setPayAmount(plan.price_rub.toString());
                    }} className="w-full text-sm">
                      <option value="">Выберите тариф</option>
                      {plans.map((p, i) => (
                        <option key={i} value={p.plan_key}>{p.name} — {p.price_rub} ₽</option>
                      ))}
                    </select>
                  )}

                  <input type="number" value={payAmount} onChange={e => setPayAmount(e.target.value)}
                    placeholder="Сумма (₽)" className="w-full" min="10" />

                  <button onClick={handleCreatePayment}
                    disabled={payLoading || !payAmount || parseFloat(payAmount) < 10}
                    className="btn btn-primary w-full py-2.5">
                    {payLoading ? 'Создание...' : 'Получить QR-код'}
                  </button>
                </div>
              ) : (
                <QRCodeView
                  payData={payData}
                  payStatus={payStatus}
                  onClose={() => { setPayModal(false); setPayData(null); setPayStatus(null); }}
                />
              )}
            </motion.div>
          </div>
        )}
      </AnimatePresence>
    </div>
  );
}
