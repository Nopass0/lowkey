'use client';

import { useState, useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useRouter } from 'next/navigation';
import {
  Shield, Users, CreditCard, Star, Tag, Zap, Check,
  X, Send, AlertTriangle, BarChart2, Lock
} from 'lucide-react';
import { useAuthStore } from '@/store/auth';
import { admin, UserPublic, SubscriptionPlan } from '@/lib/api';
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
  const [activeTab, setActiveTab] = useState<'stats' | 'users' | 'payments' | 'promos' | 'withdrawals' | 'plans'>('stats');

  // Admin data
  const [stats, setStats] = useState<any>(null);
  const [users, setUsers] = useState<UserPublic[]>([]);
  const [payments, setPayments] = useState<any[]>([]);
  const [promos, setPromos] = useState<any[]>([]);
  const [withdrawals, setWithdrawals] = useState<any[]>([]);
  const [plans, setPlans] = useState<SubscriptionPlan[]>([]);

  const loadData = async (token: string) => {
    try {
      const [s, u, pay, pro, wd, pl] = await Promise.allSettled([
        admin.stats(token),
        admin.users(token),
        admin.payments(token),
        admin.listPromos(token),
        admin.withdrawals(token),
        admin.plans(token),
      ]);
      if (s.status === 'fulfilled') setStats(s.value);
      if (u.status === 'fulfilled') setUsers(u.value.users);
      if (pay.status === 'fulfilled') setPayments(pay.value.payments);
      if (pro.status === 'fulfilled') setPromos(pro.value.promos);
      if (wd.status === 'fulfilled') setWithdrawals(wd.value.withdrawals);
      if (pl.status === 'fulfilled') setPlans(pl.value.plans);
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
              style={{ background: 'linear-gradient(135deg, #00ff88, #0066ff)' }}>
              <Lock className="w-6 h-6 text-black" />
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
              style={{ background: 'linear-gradient(135deg, #00ff88, #0066ff)' }}>
              <Shield className="w-4 h-4 text-black" />
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
          ].map(tab => (
            <button key={tab.id} onClick={() => setActiveTab(tab.id as any)}
              className={`flex items-center gap-2 px-4 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors whitespace-nowrap ${
                activeTab === tab.id ? 'border-green-400 text-white' : 'border-transparent'
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
        <Icon className="w-5 h-5" style={{ color: warning ? '#ffa500' : '#00ff88' }} />
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
                    className="text-xs" style={{ color: '#00ff88' }}>
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

function PromosTab({ promos, adminToken, onRefresh }: { promos: any[]; adminToken: string; onRefresh: () => void }) {
  const [form, setForm] = useState({ code: '', type: 'balance', value: '', extra: '', max_uses: '1', expires_days: '' });
  const [loading, setLoading] = useState(false);

  const handleCreate = async () => {
    setLoading(true);
    try {
      await admin.createPromo(adminToken, {
        code: form.code,
        type: form.type,
        value: parseFloat(form.value),
        extra: form.extra ? parseFloat(form.extra) : undefined,
        max_uses: parseInt(form.max_uses) || 1,
        expires_days: form.expires_days ? parseInt(form.expires_days) : undefined,
      });
      onRefresh();
      setForm({ code: '', type: 'balance', value: '', extra: '', max_uses: '1', expires_days: '' });
    } catch (e: any) { alert(e.message); }
    finally { setLoading(false); }
  };

  return (
    <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} className="space-y-6">
      {/* Create promo form */}
      <div className="glass rounded-2xl p-6">
        <h3 className="font-semibold mb-4">Создать промокод</h3>
        <div className="grid grid-cols-2 md:grid-cols-3 gap-4">
          <input value={form.code} onChange={e => setForm({...form, code: e.target.value.toUpperCase()})}
            placeholder="Код" className="col-span-1" />
          <select value={form.type} onChange={e => setForm({...form, type: e.target.value})}>
            <option value="balance">Баланс (₽)</option>
            <option value="discount">Скидка (%)</option>
            <option value="free_days">Бесплатные дни</option>
            <option value="speed">Скорость</option>
          </select>
          <input type="number" value={form.value} onChange={e => setForm({...form, value: e.target.value})}
            placeholder="Значение" />
          {form.type === 'speed' && (
            <input type="number" value={form.extra} onChange={e => setForm({...form, extra: e.target.value})}
              placeholder="Дней (для speed)" />
          )}
          <input type="number" value={form.max_uses} onChange={e => setForm({...form, max_uses: e.target.value})}
            placeholder="Макс. использований" />
          <input type="number" value={form.expires_days} onChange={e => setForm({...form, expires_days: e.target.value})}
            placeholder="Срок действия (дней)" />
        </div>
        <button onClick={handleCreate} disabled={loading || !form.code || !form.value}
          className="btn btn-primary mt-4">
          Создать
        </button>
      </div>

      {/* Promos list */}
      <div className="glass rounded-2xl overflow-hidden">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b" style={{ borderColor: 'var(--border)' }}>
              {['Код', 'Тип', 'Значение', 'Использований', 'Истекает', ''].map(h => (
                <th key={h} className="text-left px-4 py-3 font-medium" style={{ color: 'var(--muted-foreground)' }}>{h}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {promos.map(p => (
              <tr key={p.id} className="border-b hover:bg-white/2" style={{ borderColor: 'var(--border)' }}>
                <td className="px-4 py-3 font-mono font-semibold">{p.code}</td>
                <td className="px-4 py-3 text-xs badge badge-pending">{p.type}</td>
                <td className="px-4 py-3">{p.value}{p.extra > 0 ? ` / ${p.extra}д` : ''}</td>
                <td className="px-4 py-3">{p.used_count}/{p.max_uses}</td>
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
