'use client';

import { useState, useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useRouter } from 'next/navigation';
import {
  Shield, Users, CreditCard, Star, Tag, Zap, Check,
  X, Send, AlertTriangle, BarChart2, Lock, Package, Download,
  Trash2, Globe, Hash, Plus,
} from 'lucide-react';
import { useAuthStore } from '@/store/auth';
import { admin, UserPublic, SubscriptionPlan, AppRelease } from '@/lib/api';
import { formatRub, formatDate, formatDateTime, getStatusLabel } from '@/lib/utils';

export default function AdminPage() {
  const router = useRouter();
  const { adminToken, setAdminToken } = useAuthStore();
  const [step, setStep] = useState<'request' | 'verify' | 'dashboard'>(
    adminToken ? 'dashboard' : 'request'
  );
  const [code, setCode] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [activeTab, setActiveTab] = useState<'stats' | 'users' | 'payments' | 'promos' | 'withdrawals' | 'plans' | 'releases'>('stats');

  // Admin data
  const [stats, setStats] = useState<any>(null);
  const [users, setUsers] = useState<UserPublic[]>([]);
  const [payments, setPayments] = useState<any[]>([]);
  const [promos, setPromos] = useState<any[]>([]);
  const [withdrawals, setWithdrawals] = useState<any[]>([]);
  const [plans, setPlans] = useState<SubscriptionPlan[]>([]);
  const [releases, setReleases] = useState<AppRelease[]>([]);

  const loadData = async (token: string) => {
    try {
      const [s, u, pay, pro, wd, pl, rel] = await Promise.allSettled([
        admin.stats(token),
        admin.users(token),
        admin.payments(token),
        admin.listPromos(token),
        admin.withdrawals(token),
        admin.plans(token),
        admin.releases(token),
      ]);
      if (s.status === 'fulfilled') setStats(s.value);
      if (u.status === 'fulfilled') setUsers(u.value.users);
      if (pay.status === 'fulfilled') setPayments(pay.value.payments);
      if (pro.status === 'fulfilled') setPromos(pro.value.promos);
      if (wd.status === 'fulfilled') setWithdrawals(wd.value.withdrawals);
      if (pl.status === 'fulfilled') setPlans(pl.value.plans);
      if (rel.status === 'fulfilled') setReleases(rel.value.releases);
    } catch (e) {}
  };

  useEffect(() => {
    if (adminToken) {
      setStep('dashboard');
      loadData(adminToken);
    }
  }, [adminToken]);

  const handleRequestCode = async () => {
    setLoading(true);
    setError('');
    try {
      await admin.requestCode();
      setStep('verify');
    } catch (e: any) {
      setError(e.message || 'Ошибка');
    } finally {
      setLoading(false);
    }
  };

  const handleVerify = async () => {
    setLoading(true);
    setError('');
    try {
      const res = await admin.verifyCode(code);
      setAdminToken(res.token);
      setStep('dashboard');
    } catch (e: any) {
      setError('Неверный или устаревший код');
    } finally {
      setLoading(false);
    }
  };

  // Auth screens
  if (step !== 'dashboard') {
    return (
      <div className="min-h-screen flex items-center justify-center px-4" style={{ background: 'var(--background)' }}>
        <motion.div initial={{ opacity: 0, y: 30 }} animate={{ opacity: 1, y: 0 }}
          className="glass rounded-3xl p-8 w-full max-w-sm">
          <div className="text-center mb-8">
            <div className="w-12 h-12 rounded-xl mx-auto mb-4 flex items-center justify-center"
              style={{ background: 'linear-gradient(135deg, #3b82f6, #2563eb)' }}>
              <Lock className="w-6 h-6 text-white" />
            </div>
            <h1 className="text-2xl font-bold">Панель администратора</h1>
          </div>

          {error && (
            <div className="rounded-xl p-3 mb-4 text-sm"
              style={{ background: 'rgba(255,68,68,0.1)', color: '#ff4444' }}>
              {error}
            </div>
          )}

          {step === 'request' ? (
            <div>
              <p className="text-sm mb-6 text-center" style={{ color: 'var(--muted-foreground)' }}>
                Код подтверждения будет отправлен в Telegram-бот администратора
              </p>
              <button onClick={handleRequestCode} disabled={loading}
                className="btn btn-primary w-full py-3">
                {loading ? 'Отправка...' : 'Запросить код'}
              </button>
            </div>
          ) : (
            <div>
              <p className="text-sm mb-4 text-center" style={{ color: 'var(--muted-foreground)' }}>
                Введите 6-значный код из Telegram
              </p>
              <input type="text" value={code} onChange={e => setCode(e.target.value)}
                placeholder="000000" maxLength={6} className="w-full mb-4 text-center text-2xl"
                style={{ letterSpacing: '0.5em' }} />
              <div className="flex gap-3">
                <button onClick={() => setStep('request')} className="btn btn-secondary flex-1">Назад</button>
                <button onClick={handleVerify} disabled={loading || code.length !== 6}
                  className="btn btn-primary flex-1">Войти</button>
              </div>
            </div>
          )}
        </motion.div>
      </div>
    );
  }

  return (
    <div className="min-h-screen" style={{ background: 'var(--background)' }}>
      {/* Header */}
      <header className="glass border-b sticky top-0 z-40" style={{ borderColor: 'var(--border)' }}>
        <div className="max-w-7xl mx-auto px-4 h-16 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <div className="w-8 h-8 rounded-lg flex items-center justify-center"
              style={{ background: 'linear-gradient(135deg, #3b82f6, #2563eb)' }}>
              <Shield className="w-4 h-4 text-white" />
            </div>
            <span className="font-bold gradient-text">Lowkey VPN Admin</span>
          </div>
          <div className="flex items-center gap-3">
            <button onClick={() => router.push('/dashboard')} className="btn btn-secondary text-sm">
              Личный кабинет
            </button>
            <button onClick={() => { setAdminToken(''); setStep('request'); }}
              className="btn btn-secondary text-sm">
              Выйти
            </button>
          </div>
        </div>
      </header>

      {/* Tabs */}
      <div className="border-b" style={{ borderColor: 'var(--border)' }}>
        <div className="max-w-7xl mx-auto px-4 flex gap-1 pt-2 overflow-x-auto">
          {[
            { id: 'stats', label: 'Статистика', icon: BarChart2 },
            { id: 'users', label: 'Пользователи', icon: Users },
            { id: 'payments', label: 'Платежи', icon: CreditCard },
            { id: 'promos', label: 'Промокоды', icon: Tag },
            { id: 'withdrawals', label: 'Выводы', icon: Send },
            { id: 'plans', label: 'Тарифы', icon: Zap },
            { id: 'releases', label: 'Релизы', icon: Package },
          ].map(tab => (
            <button key={tab.id} onClick={() => setActiveTab(tab.id as any)}
              className={`flex items-center gap-2 px-4 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors whitespace-nowrap ${
                activeTab === tab.id ? 'border-blue-400 text-white' : 'border-transparent'
              }`}
              style={{ color: activeTab === tab.id ? 'var(--foreground)' : 'var(--muted-foreground)' }}>
              <tab.icon className="w-4 h-4" />
              {tab.label}
            </button>
          ))}
        </div>
      </div>

      <main className="max-w-7xl mx-auto px-4 py-8">
        {/* Stats Tab */}
        {activeTab === 'stats' && stats && (
          <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }}
            className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            <StatCard icon={Users} label="Пользователей" value={stats.total_users} />
            <StatCard icon={Shield} label="Активных подписок" value={stats.active_subscriptions} />
            <StatCard icon={CreditCard} label="Выручка" value={formatRub(stats.total_revenue_rub)} />
            <StatCard icon={Send} label="Ожидают выплаты рефералам" value={formatRub(stats.pending_referral_payouts_rub)}
              warning={stats.pending_referral_payouts_rub > 0} />
            <StatCard icon={Star} label="Заморожено под рефералов" value={formatRub(stats.total_referral_balance_frozen_rub)} />
          </motion.div>
        )}

        {/* Users Tab */}
        {activeTab === 'users' && (
          <UsersTab users={users} adminToken={adminToken!} onRefresh={() => loadData(adminToken!)} />
        )}

        {/* Payments Tab */}
        {activeTab === 'payments' && (
          <PaymentsTab payments={payments} adminToken={adminToken!} onRefresh={() => loadData(adminToken!)} />
        )}

        {/* Promos Tab */}
        {activeTab === 'promos' && (
          <PromosTab promos={promos} adminToken={adminToken!} onRefresh={() => loadData(adminToken!)} />
        )}

        {/* Withdrawals Tab */}
        {activeTab === 'withdrawals' && (
          <WithdrawalsTab withdrawals={withdrawals} adminToken={adminToken!} onRefresh={() => loadData(adminToken!)} />
        )}

        {/* Plans Tab */}
        {activeTab === 'plans' && (
          <PlansTab plans={plans} adminToken={adminToken!} onRefresh={() => loadData(adminToken!)} />
        )}

        {/* Releases Tab */}
        {activeTab === 'releases' && (
          <ReleasesTab releases={releases} adminToken={adminToken!} onRefresh={() => loadData(adminToken!)} />
        )}
      </main>
    </div>
  );
}

function StatCard({ icon: Icon, label, value, warning = false }: {
  icon: any; label: string; value: any; warning?: boolean;
}) {
  return (
    <div className={`glass rounded-2xl p-6 ${warning ? 'ring-1 ring-orange-400/50' : ''}`}>
      <div className="flex items-center gap-2 mb-3">
        <Icon className="w-5 h-5" style={{ color: warning ? '#ffa500' : '#60a5fa' }} />
        <span className="text-sm" style={{ color: 'var(--muted-foreground)' }}>{label}</span>
      </div>
      <div className={`text-3xl font-bold ${warning ? '' : 'gradient-text'}`}
        style={warning ? { color: '#ffa500' } : {}}>
        {value}
      </div>
    </div>
  );
}

function UsersTab({ users, adminToken, onRefresh }: { users: UserPublic[]; adminToken: string; onRefresh: () => void }) {
  const [editUser, setEditUser] = useState<number | null>(null);
  const [limitVal, setLimitVal] = useState('');

  return (
    <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }}
      className="glass rounded-2xl overflow-hidden">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b" style={{ borderColor: 'var(--border)' }}>
            {['ID', 'Логин', 'Баланс', 'Подписка', 'Рефбаланс', 'Лимит', 'Действия'].map(h => (
              <th key={h} className="text-left px-4 py-3 font-medium" style={{ color: 'var(--muted-foreground)' }}>{h}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {users.map(u => (
            <tr key={u.id} className="border-b hover:bg-white/2 transition-colors" style={{ borderColor: 'var(--border)' }}>
              <td className="px-4 py-3" style={{ color: 'var(--muted-foreground)' }}>{u.id}</td>
              <td className="px-4 py-3 font-medium">{u.login}
                {u.role === 'admin' && <span className="badge badge-active ml-2 text-xs">admin</span>}
                {u.role === 'banned' && <span className="badge badge-expired ml-2 text-xs">banned</span>}
              </td>
              <td className="px-4 py-3">{formatRub(u.balance)}</td>
              <td className="px-4 py-3">
                <span className={`badge ${u.sub_status === 'active' ? 'badge-active' : 'badge-expired'}`}>
                  {getStatusLabel(u.sub_status)}
                </span>
                {u.sub_expires_at && (
                  <div className="text-xs mt-1" style={{ color: 'var(--muted-foreground)' }}>
                    до {formatDate(u.sub_expires_at)}
                  </div>
                )}
              </td>
              <td className="px-4 py-3">{formatRub(u.referral_balance)}</td>
              <td className="px-4 py-3">
                {editUser === u.id ? (
                  <div className="flex gap-2">
                    <input type="number" value={limitVal} onChange={e => setLimitVal(e.target.value)}
                      className="w-20 py-1 px-2 text-xs" placeholder="Mbps" />
                    <button onClick={async () => {
                      await admin.setUserLimit(adminToken, u.id, parseFloat(limitVal) || 0);
                      setEditUser(null); onRefresh();
                    }} className="btn btn-primary text-xs py-1 px-2"><Check className="w-3 h-3" /></button>
                    <button onClick={() => setEditUser(null)} className="btn btn-secondary text-xs py-1 px-2"><X className="w-3 h-3" /></button>
                  </div>
                ) : (
                  <button onClick={() => { setEditUser(u.id); setLimitVal(u.sub_speed_mbps.toString()); }}
                    className="text-xs" style={{ color: '#60a5fa' }}>
                    {u.sub_speed_mbps === 0 ? '∞' : `${u.sub_speed_mbps} Мб`}
                  </button>
                )}
              </td>
              <td className="px-4 py-3">
                <button
                  onClick={async () => {
                    await admin.banUser(adminToken, u.id, u.role !== 'banned');
                    onRefresh();
                  }}
                  className={`btn text-xs py-1 px-3 ${u.role === 'banned' ? 'btn-primary' : 'btn-secondary'}`}>
                  {u.role === 'banned' ? 'Разбан' : 'Бан'}
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </motion.div>
  );
}

function PaymentsTab({ payments, adminToken, onRefresh }: { payments: any[]; adminToken: string; onRefresh: () => void }) {
  return (
    <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }}
      className="glass rounded-2xl overflow-hidden">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b" style={{ borderColor: 'var(--border)' }}>
            {['ID', 'Пользователь', 'Сумма', 'Тип', 'Статус', 'Дата', ''].map(h => (
              <th key={h} className="text-left px-4 py-3 font-medium" style={{ color: 'var(--muted-foreground)' }}>{h}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {payments.map(p => (
            <tr key={p.id} className="border-b hover:bg-white/2" style={{ borderColor: 'var(--border)' }}>
              <td className="px-4 py-3 text-xs" style={{ color: 'var(--muted-foreground)' }}>#{p.id}</td>
              <td className="px-4 py-3">user #{p.user_id}</td>
              <td className="px-4 py-3 font-semibold">{formatRub(p.amount)}</td>
              <td className="px-4 py-3 text-xs">{p.purpose}{p.plan_id ? ` (${p.plan_id})` : ''}</td>
              <td className="px-4 py-3">
                <span className={`badge ${p.status === 'paid' ? 'badge-active' : p.status === 'pending' ? 'badge-pending' : 'badge-expired'}`}>
                  {getStatusLabel(p.status)}
                </span>
              </td>
              <td className="px-4 py-3 text-xs" style={{ color: 'var(--muted-foreground)' }}>
                {formatDateTime(p.created_at)}
              </td>
              <td className="px-4 py-3">
                {p.status === 'pending' && (
                  <button onClick={async () => {
                    await admin.confirmPayment(adminToken, p.id);
                    onRefresh();
                  }} className="btn btn-primary text-xs py-1 px-3">Подтвердить</button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </motion.div>
  );
}

const PROMO_TYPES = [
  { value: 'balance',      label: 'Баланс (₽)' },
  { value: 'discount',     label: 'Скидка (%)' },
  { value: 'free_days',    label: 'Бесплатные дни' },
  { value: 'speed',        label: 'Скорость (Мб/с)' },
  { value: 'subscription', label: 'Подписка (дней)' },
  { value: 'combo',        label: 'Комбо (баланс + дни)' },
];

const SECOND_TYPES = [
  { value: '',          label: 'Нет' },
  { value: 'balance',   label: 'Баланс (₽)' },
  { value: 'free_days', label: 'Бесплатные дни' },
  { value: 'speed',     label: 'Скорость (Мб/с)' },
];

const EMPTY_PROMO = {
  code: '', type: 'balance', value: '', extra: '', max_uses: '1', expires_days: '',
  description: '', target_user_id: '', only_new_users: false,
  min_purchase_rub: '', second_type: '', second_value: '', max_uses_per_user: '1',
};

function PromosTab({ promos, adminToken, onRefresh }: { promos: any[]; adminToken: string; onRefresh: () => void }) {
  const [form, setForm] = useState({ ...EMPTY_PROMO });
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [loading, setLoading] = useState(false);

  const f = (k: string, v: any) => setForm(p => ({ ...p, [k]: v }));

  const needsExtra = ['speed', 'subscription', 'combo'].includes(form.type);

  const handleCreate = async () => {
    setLoading(true);
    try {
      await admin.createPromo(adminToken, {
        code: form.code || undefined,
        type: form.type,
        value: parseFloat(form.value),
        extra: form.extra ? parseFloat(form.extra) : undefined,
        max_uses: parseInt(form.max_uses) || 1,
        expires_days: form.expires_days ? parseInt(form.expires_days) : undefined,
        description: form.description || undefined,
        target_user_id: form.target_user_id ? parseInt(form.target_user_id) : undefined,
        only_new_users: form.only_new_users,
        min_purchase_rub: form.min_purchase_rub ? parseFloat(form.min_purchase_rub) : undefined,
        second_type: form.second_type || undefined,
        second_value: form.second_value ? parseFloat(form.second_value) : undefined,
        max_uses_per_user: parseInt(form.max_uses_per_user) || 1,
      });
      onRefresh();
      setForm({ ...EMPTY_PROMO });
    } catch (e: any) { alert(e.message); }
    finally { setLoading(false); }
  };

  return (
    <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} className="space-y-6">
      {/* Create promo form */}
      <div className="glass rounded-2xl p-6">
        <h3 className="font-semibold mb-4">Создать промокод</h3>

        {/* Basic fields */}
        <div className="grid grid-cols-2 md:grid-cols-3 gap-4 mb-4">
          <div>
            <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>Код (пусто = авто)</label>
            <input value={form.code} onChange={e => f('code', e.target.value.toUpperCase())} placeholder="AUTO" />
          </div>
          <div>
            <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>Тип</label>
            <select value={form.type} onChange={e => f('type', e.target.value)} className="w-full">
              {PROMO_TYPES.map(t => <option key={t.value} value={t.value}>{t.label}</option>)}
            </select>
          </div>
          <div>
            <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>
              {form.type === 'balance' ? 'Сумма (₽)' : form.type === 'discount' ? 'Скидка (%)' :
               form.type === 'free_days' ? 'Дней' : form.type === 'speed' ? 'Мб/с' :
               form.type === 'subscription' ? 'Дней' : 'Баланс (₽)'}
            </label>
            <input type="number" value={form.value} onChange={e => f('value', e.target.value)} placeholder="0" />
          </div>
          {needsExtra && (
            <div>
              <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>
                {form.type === 'speed' ? 'Дней' : form.type === 'combo' ? 'Дней' : 'Макс. скорость (Мб/с)'}
              </label>
              <input type="number" value={form.extra} onChange={e => f('extra', e.target.value)} placeholder="0" />
            </div>
          )}
          <div>
            <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>Макс. использований (0=∞)</label>
            <input type="number" value={form.max_uses} onChange={e => f('max_uses', e.target.value)} placeholder="1" />
          </div>
          <div>
            <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>Срок действия (дней)</label>
            <input type="number" value={form.expires_days} onChange={e => f('expires_days', e.target.value)} placeholder="∞" />
          </div>
          <div className="col-span-full">
            <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>Описание (для администратора)</label>
            <input value={form.description} onChange={e => f('description', e.target.value)} placeholder="Промо на Новый год..." />
          </div>
        </div>

        {/* Advanced toggle */}
        <button onClick={() => setShowAdvanced(v => !v)}
          className="text-sm mb-4 flex items-center gap-1"
          style={{ color: '#60a5fa' }}>
          <Plus className={`w-3 h-3 transition-transform ${showAdvanced ? 'rotate-45' : ''}`} />
          {showAdvanced ? 'Скрыть условия' : 'Дополнительные условия'}
        </button>

        {showAdvanced && (
          <div className="grid grid-cols-2 md:grid-cols-3 gap-4 mb-4 p-4 rounded-xl"
            style={{ background: 'rgba(59,130,246,0.04)', border: '1px solid rgba(59,130,246,0.15)' }}>
            <div>
              <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>Только для user_id</label>
              <input type="number" value={form.target_user_id} onChange={e => f('target_user_id', e.target.value)}
                placeholder="Любой" />
            </div>
            <div>
              <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>Мин. покупка (₽)</label>
              <input type="number" value={form.min_purchase_rub} onChange={e => f('min_purchase_rub', e.target.value)}
                placeholder="0" />
            </div>
            <div>
              <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>Макс. использований на юзера</label>
              <input type="number" value={form.max_uses_per_user} onChange={e => f('max_uses_per_user', e.target.value)}
                placeholder="1" />
            </div>
            <div>
              <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>Доп. тип эффекта</label>
              <select value={form.second_type} onChange={e => f('second_type', e.target.value)} className="w-full">
                {SECOND_TYPES.map(t => <option key={t.value} value={t.value}>{t.label}</option>)}
              </select>
            </div>
            {form.second_type && (
              <div>
                <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>Доп. значение</label>
                <input type="number" value={form.second_value} onChange={e => f('second_value', e.target.value)} placeholder="0" />
              </div>
            )}
            <div className="flex items-center gap-2 col-span-full mt-1">
              <input type="checkbox" id="only_new" checked={form.only_new_users} onChange={e => f('only_new_users', e.target.checked)}
                className="w-4 h-4" />
              <label htmlFor="only_new" className="text-sm cursor-pointer">Только для новых пользователей</label>
            </div>
          </div>
        )}

        <button onClick={handleCreate} disabled={loading || !form.value}
          className="btn btn-primary">
          {loading ? 'Создание...' : 'Создать промокод'}
        </button>
      </div>

      {/* Promos list */}
      <div className="glass rounded-2xl overflow-hidden">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b" style={{ borderColor: 'var(--border)' }}>
              {['Код', 'Тип', 'Значение', 'Условия', 'Использований', 'Истекает', ''].map(h => (
                <th key={h} className="text-left px-4 py-3 font-medium" style={{ color: 'var(--muted-foreground)' }}>{h}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {promos.map(p => (
              <tr key={p.id} className="border-b hover:bg-white/2" style={{ borderColor: 'var(--border)' }}>
                <td className="px-4 py-3">
                  <div className="font-mono font-semibold">{p.code}</div>
                  {p.description && <div className="text-xs mt-0.5" style={{ color: 'var(--muted-foreground)' }}>{p.description}</div>}
                </td>
                <td className="px-4 py-3">
                  <span className="badge badge-pending text-xs">{p.type}</span>
                  {p.second_type && <span className="ml-1 badge badge-active text-xs">+{p.second_type}</span>}
                </td>
                <td className="px-4 py-3">
                  {p.value}{p.extra > 0 ? ` / ${p.extra}` : ''}
                  {p.second_value > 0 ? ` & ${p.second_value}` : ''}
                </td>
                <td className="px-4 py-3 text-xs" style={{ color: 'var(--muted-foreground)' }}>
                  {p.only_new_users && <div>🆕 Только новые</div>}
                  {p.target_user_id && <div>👤 user #{p.target_user_id}</div>}
                  {p.min_purchase_rub > 0 && <div>💰 от {p.min_purchase_rub}₽</div>}
                  {p.max_uses_per_user > 1 && <div>×{p.max_uses_per_user}/юзер</div>}
                </td>
                <td className="px-4 py-3">
                  {p.used_count}/{p.max_uses === 0 ? '∞' : p.max_uses}
                </td>
                <td className="px-4 py-3 text-xs" style={{ color: 'var(--muted-foreground)' }}>
                  {p.expires_at ? formatDate(p.expires_at) : '∞'}
                </td>
                <td className="px-4 py-3">
                  <button onClick={async () => { await admin.deletePromo(adminToken, p.id); onRefresh(); }}
                    className="btn btn-secondary text-xs py-1 px-2">
                    <X className="w-3 h-3" />
                  </button>
                </td>
              </tr>
            ))}
            {promos.length === 0 && (
              <tr><td colSpan={7} className="px-6 py-8 text-center text-sm" style={{ color: 'var(--muted-foreground)' }}>
                Нет промокодов
              </td></tr>
            )}
          </tbody>
        </table>
      </div>
    </motion.div>
  );
}

function WithdrawalsTab({ withdrawals, adminToken, onRefresh }: { withdrawals: any[]; adminToken: string; onRefresh: () => void }) {
  const [noteMap, setNoteMap] = useState<Record<number, string>>({});

  return (
    <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }}
      className="glass rounded-2xl overflow-hidden">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b" style={{ borderColor: 'var(--border)' }}>
            {['ID', 'Пользователь', 'Сумма', 'Карта', 'Банк', 'Статус', 'Дата', 'Примечание', ''].map(h => (
              <th key={h} className="text-left px-4 py-3 font-medium" style={{ color: 'var(--muted-foreground)' }}>{h}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {withdrawals.map(w => (
            <tr key={w.id} className="border-b hover:bg-white/2" style={{ borderColor: 'var(--border)' }}>
              <td className="px-4 py-3 text-xs" style={{ color: 'var(--muted-foreground)' }}>#{w.id}</td>
              <td className="px-4 py-3">user #{w.user_id}</td>
              <td className="px-4 py-3 font-semibold">{formatRub(w.amount)}</td>
              <td className="px-4 py-3 font-mono text-xs">{w.card_number}</td>
              <td className="px-4 py-3 text-xs">{w.bank_name || '—'}</td>
              <td className="px-4 py-3">
                <span className={`badge ${w.status === 'completed' ? 'badge-active' : w.status === 'pending' ? 'badge-pending' : 'badge-expired'}`}>
                  {getStatusLabel(w.status)}
                </span>
              </td>
              <td className="px-4 py-3 text-xs" style={{ color: 'var(--muted-foreground)' }}>
                {formatDate(w.requested_at)}
              </td>
              <td className="px-4 py-3">
                {w.status === 'pending' && (
                  <input type="text" value={noteMap[w.id] || ''} onChange={e => setNoteMap({...noteMap, [w.id]: e.target.value})}
                    placeholder="Примечание" className="w-32 text-xs py-1 px-2" />
                )}
                {w.admin_note && <span className="text-xs" style={{ color: 'var(--muted-foreground)' }}>{w.admin_note}</span>}
              </td>
              <td className="px-4 py-3">
                {w.status === 'pending' && (
                  <div className="flex gap-2">
                    <button onClick={async () => {
                      await admin.approveWithdrawal(adminToken, w.id, noteMap[w.id]);
                      onRefresh();
                    }} className="btn btn-primary text-xs py-1 px-2">
                      <Check className="w-3 h-3" /> Одобрить
                    </button>
                    <button onClick={async () => {
                      await admin.rejectWithdrawal(adminToken, w.id, noteMap[w.id]);
                      onRefresh();
                    }} className="btn btn-secondary text-xs py-1 px-2">
                      <X className="w-3 h-3" /> Отклонить
                    </button>
                  </div>
                )}
              </td>
            </tr>
          ))}
          {withdrawals.length === 0 && (
            <tr><td colSpan={9} className="px-6 py-8 text-center text-sm" style={{ color: 'var(--muted-foreground)' }}>
              Нет заявок на вывод
            </td></tr>
          )}
        </tbody>
      </table>
    </motion.div>
  );
}

function ReleasesTab({ releases, adminToken, onRefresh }: { releases: AppRelease[]; adminToken: string; onRefresh: () => void }) {
  const EMPTY_REL = {
    platform: 'windows', version: '', download_url: '',
    file_name: '', file_size_bytes: '', sha256_checksum: '',
    changelog: '', min_os_version: '', set_latest: true,
  };
  const [form, setForm] = useState({ ...EMPTY_REL });
  const [loading, setLoading] = useState(false);
  const f = (k: string, v: any) => setForm(p => ({ ...p, [k]: v }));

  const handleCreate = async () => {
    setLoading(true);
    try {
      await admin.createRelease(adminToken, {
        platform: form.platform,
        version: form.version,
        download_url: form.download_url,
        file_name: form.file_name || undefined,
        file_size_bytes: form.file_size_bytes ? parseInt(form.file_size_bytes) : undefined,
        sha256_checksum: form.sha256_checksum || undefined,
        changelog: form.changelog || undefined,
        min_os_version: form.min_os_version || undefined,
        set_latest: form.set_latest,
      });
      onRefresh();
      setForm({ ...EMPTY_REL });
    } catch (e: any) { alert(e.message); }
    finally { setLoading(false); }
  };

  const PLATFORMS = ['windows', 'linux', 'android', 'macos'];
  const grouped = PLATFORMS.reduce<Record<string, AppRelease[]>>((acc, p) => {
    acc[p] = releases.filter(r => r.platform === p);
    return acc;
  }, {});

  const formatSize = (bytes: number | null) => {
    if (!bytes) return '—';
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  return (
    <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} className="space-y-6">
      {/* Create release form */}
      <div className="glass rounded-2xl p-6">
        <h3 className="font-semibold mb-4 flex items-center gap-2">
          <Package className="w-4 h-4" style={{ color: '#60a5fa' }} />
          Добавить релиз
        </h3>
        <div className="grid grid-cols-2 md:grid-cols-3 gap-4 mb-4">
          <div>
            <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>Платформа</label>
            <select value={form.platform} onChange={e => f('platform', e.target.value)} className="w-full">
              <option value="windows">Windows</option>
              <option value="linux">Linux</option>
              <option value="android">Android</option>
              <option value="macos">macOS</option>
            </select>
          </div>
          <div>
            <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>Версия</label>
            <input value={form.version} onChange={e => f('version', e.target.value)} placeholder="1.0.0" />
          </div>
          <div>
            <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>Мин. ОС</label>
            <input value={form.min_os_version} onChange={e => f('min_os_version', e.target.value)} placeholder="10" />
          </div>
          <div className="col-span-full">
            <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>URL для скачивания</label>
            <input value={form.download_url} onChange={e => f('download_url', e.target.value)} placeholder="https://..." className="w-full" />
          </div>
          <div>
            <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>Имя файла</label>
            <input value={form.file_name} onChange={e => f('file_name', e.target.value)} placeholder="lowkey-setup-1.0.0.exe" />
          </div>
          <div>
            <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>Размер (байт)</label>
            <input type="number" value={form.file_size_bytes} onChange={e => f('file_size_bytes', e.target.value)} placeholder="0" />
          </div>
          <div>
            <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>SHA256</label>
            <input value={form.sha256_checksum} onChange={e => f('sha256_checksum', e.target.value)} placeholder="abc123..." className="font-mono text-xs" />
          </div>
          <div className="col-span-full">
            <label className="text-xs mb-1 block" style={{ color: 'var(--muted-foreground)' }}>Changelog</label>
            <textarea value={form.changelog} onChange={e => f('changelog', e.target.value)}
              placeholder="- Исправлены баги&#10;- Улучшена производительность"
              rows={3} className="w-full resize-none"
              style={{ background: 'rgba(255,255,255,0.05)', borderRadius: 8, padding: '8px 12px', border: '1px solid var(--border)', color: 'var(--foreground)', fontSize: 13 }} />
          </div>
          <div className="flex items-center gap-2">
            <input type="checkbox" id="set_latest" checked={form.set_latest} onChange={e => f('set_latest', e.target.checked)} className="w-4 h-4" />
            <label htmlFor="set_latest" className="text-sm cursor-pointer">Сделать текущей</label>
          </div>
        </div>
        <button onClick={handleCreate} disabled={loading || !form.version || !form.download_url}
          className="btn btn-primary flex items-center gap-2">
          <Plus className="w-4 h-4" />
          {loading ? 'Добавление...' : 'Добавить релиз'}
        </button>
      </div>

      {/* Releases list by platform */}
      {PLATFORMS.map(platform => {
        const platformReleases = grouped[platform];
        if (platformReleases.length === 0) return null;
        return (
          <div key={platform} className="glass rounded-2xl overflow-hidden">
            <div className="px-6 py-4 border-b flex items-center gap-2" style={{ borderColor: 'var(--border)' }}>
              <Globe className="w-4 h-4" style={{ color: '#60a5fa' }} />
              <span className="font-semibold capitalize">{platform}</span>
              <span className="text-sm" style={{ color: 'var(--muted-foreground)' }}>
                ({platformReleases.length} {platformReleases.length === 1 ? 'релиз' : 'релизов'})
              </span>
            </div>
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b" style={{ borderColor: 'var(--border)' }}>
                  {['Версия', 'Файл', 'Размер', 'SHA256', 'Дата', 'Статус', 'Действия'].map(h => (
                    <th key={h} className="text-left px-4 py-3 font-medium" style={{ color: 'var(--muted-foreground)' }}>{h}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {platformReleases.map(r => (
                  <tr key={r.id} className="border-b hover:bg-white/2" style={{ borderColor: 'var(--border)' }}>
                    <td className="px-4 py-3 font-mono font-semibold">v{r.version}</td>
                    <td className="px-4 py-3 text-xs">
                      <a href={r.download_url} target="_blank" rel="noreferrer"
                        className="flex items-center gap-1 hover:underline" style={{ color: '#0066ff' }}>
                        <Download className="w-3 h-3" />
                        {r.file_name || 'Скачать'}
                      </a>
                    </td>
                    <td className="px-4 py-3 text-xs">{formatSize(r.file_size_bytes)}</td>
                    <td className="px-4 py-3 text-xs font-mono max-w-[120px] truncate"
                      title={r.sha256_checksum || ''} style={{ color: 'var(--muted-foreground)' }}>
                      {r.sha256_checksum ? (
                        <span className="flex items-center gap-1">
                          <Hash className="w-3 h-3 flex-shrink-0" />
                          {r.sha256_checksum.substring(0, 12)}…
                        </span>
                      ) : '—'}
                    </td>
                    <td className="px-4 py-3 text-xs" style={{ color: 'var(--muted-foreground)' }}>
                      {new Date(r.released_at).toLocaleDateString('ru-RU')}
                    </td>
                    <td className="px-4 py-3">
                      {r.is_latest && (
                        <span className="badge badge-active flex items-center gap-1 w-fit">
                          <Star className="w-3 h-3" /> Текущая
                        </span>
                      )}
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex items-center gap-2">
                        {!r.is_latest && (
                          <button onClick={async () => { await admin.setReleaseLatest(adminToken, r.id); onRefresh(); }}
                            className="btn btn-secondary text-xs py-1 px-2 flex items-center gap-1">
                            <Star className="w-3 h-3" /> Текущая
                          </button>
                        )}
                        <button onClick={async () => {
                          if (confirm(`Удалить v${r.version}?`)) {
                            await admin.deleteRelease(adminToken, r.id);
                            onRefresh();
                          }
                        }} className="btn btn-secondary text-xs py-1 px-2">
                          <Trash2 className="w-3 h-3" />
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        );
      })}

      {releases.length === 0 && (
        <div className="glass rounded-2xl p-12 text-center">
          <Package className="w-12 h-12 mx-auto mb-4 opacity-30" />
          <div className="text-sm" style={{ color: 'var(--muted-foreground)' }}>Нет загруженных релизов</div>
        </div>
      )}
    </motion.div>
  );
}

function PlansTab({ plans, adminToken, onRefresh }: { plans: SubscriptionPlan[]; adminToken: string; onRefresh: () => void }) {
  const [prices, setPrices] = useState<Record<string, string>>({});

  return (
    <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }}
      className="glass rounded-2xl overflow-hidden">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b" style={{ borderColor: 'var(--border)' }}>
            {['Тариф', 'Название', 'Цена', 'Дней', 'Скорость', 'Пакет', ''].map(h => (
              <th key={h} className="text-left px-4 py-3 font-medium" style={{ color: 'var(--muted-foreground)' }}>{h}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {plans.map((p, i) => {
            const key = p.plan_key || String(i);
            return (
              <tr key={i} className="border-b hover:bg-white/2" style={{ borderColor: 'var(--border)' }}>
                <td className="px-4 py-3 font-mono text-xs">{key}</td>
                <td className="px-4 py-3">{p.name}</td>
                <td className="px-4 py-3">
                  <div className="flex items-center gap-2">
                    <input type="number" value={prices[key] ?? p.price_rub}
                      onChange={e => setPrices({...prices, [key]: e.target.value})}
                      className="w-24 py-1 px-2 text-sm" />
                    <button onClick={async () => {
                      const price = parseFloat(prices[key] ?? String(p.price_rub));
                      await admin.updatePlanPrice(adminToken, key, price);
                      onRefresh();
                    }} className="btn btn-primary text-xs py-1 px-2">
                      <Check className="w-3 h-3" />
                    </button>
                  </div>
                </td>
                <td className="px-4 py-3">{p.duration_days}</td>
                <td className="px-4 py-3">{p.speed_mbps === 0 ? '∞' : `${p.speed_mbps} Мб/с`}</td>
                <td className="px-4 py-3">
                  {p.is_bundle ? <span className="badge badge-pending">−{p.discount_pct}%</span> : '—'}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </motion.div>
  );
}
