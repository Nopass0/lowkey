'use client';

import { motion } from 'framer-motion';
import Link from 'next/link';
import { useEffect, useState } from 'react';
import {
  Shield, Zap, Globe, Lock, Star, Download, ChevronRight,
  Check, ArrowRight, Wifi, Eye, Server, Clock, Users, Award,
  Smartphone, Monitor, HelpCircle, ChevronDown,
} from 'lucide-react';
import { subscription, versions, SubscriptionPlan, AppRelease } from '@/lib/api';

const fadeUp = {
  hidden:  { opacity: 0, y: 30 },
  visible: { opacity: 1, y: 0, transition: { duration: 0.55 } },
};
const stagger = { visible: { transition: { staggerChildren: 0.12 } } };

const FEATURES = [
  { icon: Shield, title: 'AES-256 + X25519',      desc: 'Военное шифрование, согласование ключей по X25519 Diffie–Hellman' },
  { icon: Zap,    title: 'До 500+ Мбит/с',         desc: 'QUIC-туннель с Hysteria2 — быстрее чем WireGuard на плохих каналах' },
  { icon: Globe,  title: 'Обход любых блокировок', desc: 'Маскируется под HTTPS — не детектируется DPI и системами фильтрации' },
  { icon: Eye,    title: 'Нет логов',               desc: 'Мы не храним данные о вашей активности и не передаём третьим лицам' },
  { icon: Server, title: 'Несколько протоколов',    desc: 'UDP-туннель, WebSocket и Hysteria2 QUIC — всегда найдём способ подключиться' },
  { icon: Lock,   title: 'Kill Switch',             desc: 'При обрыве VPN трафик блокируется — ни один пакет не уйдёт в открытую сеть' },
];

const HOW_IT_WORKS = [
  { num: '01', title: 'Скачайте приложение',     desc: 'Windows, Linux или Android — выберите свою платформу' },
  { num: '02', title: 'Зарегистрируйтесь',        desc: 'Один клик — аккаунт создан, пробный период начался' },
  { num: '03', title: 'Оплатите через СБП',       desc: 'Быстро и безопасно через СБП — QR-код, любой банк' },
  { num: '04', title: 'Нажмите «Подключиться»',   desc: 'Весь трафик зашифрован, IP сменён. Готово' },
];

const FAQ: { q: string; a: string }[] = [
  { q: 'Какие устройства поддерживаются?',
    a: 'Windows 10/11 (десктоп-приложение с графическим интерфейсом), Linux (CLI), Android 8+. Поддержка macOS и iOS запланирована.' },
  { q: 'Как работает оплата?',
    a: 'Оплата через Систему Быстрых Платежей (СБП). Вам показывается QR-код — оплатите его в мобильном приложении своего банка. Зачисление мгновенное.' },
  { q: 'Могу ли я получить возврат?',
    a: 'Да, если VPN не заработал у вас в течение первых 24 часов — напишите в поддержку, вернём деньги полностью.' },
  { q: 'Безопасно ли это?',
    a: 'Шифрование AES-256-GCM с X25519 ECDH. Код клиента открытый (см. репозиторий). Мы не ведём логи соединений.' },
  { q: 'Что такое реферальная программа?',
    a: 'Поделитесь своим кодом — получайте 25% с каждого платежа приглашённого пользователя навсегда. Вывод на карту от 100 ₽.' },
  { q: 'Чем отличается Hysteria2 от WireGuard?',
    a: 'Hysteria2 работает поверх QUIC (HTTP/3) и лучше работает в нестабильных сетях, на мобильных и при высоком пакетном лоссе. WireGuard быстрее на стабильных каналах.' },
];

export default function LandingPage() {
  const [plans, setPlans]       = useState<SubscriptionPlan[]>([]);
  const [releases, setReleases] = useState<AppRelease[]>([]);
  const [openFaq, setOpenFaq]   = useState<number | null>(null);

  useEffect(() => {
    subscription.plans().then(r => setPlans(r.plans)).catch(() => {});
    versions.all().then(r => setReleases(r.releases)).catch(() => {});
  }, []);

  const downloadFor = (platform: string) =>
    releases.find(r => r.platform === platform)?.download_url || '/downloads';

  return (
    <div className="min-h-screen" style={{ background: 'var(--background)' }}>
      {/* ── Navigation ──────────────────────────────────────────────────── */}
      <nav className="fixed top-0 inset-x-0 z-50 glass border-b" style={{ borderColor: 'var(--border)' }}>
        <div className="max-w-6xl mx-auto px-4 h-16 flex items-center justify-between">
          <Link href="/" className="flex items-center gap-2">
            <div className="w-8 h-8 rounded-lg flex items-center justify-center"
              style={{ background: 'linear-gradient(135deg,#00ff88,#0066ff)' }}>
              <Shield className="w-4 h-4 text-black" />
            </div>
            <span className="font-bold text-lg gradient-text">Lowkey VPN</span>
          </Link>
          <div className="hidden md:flex items-center gap-6 text-sm" style={{ color: 'var(--muted-foreground)' }}>
            <a href="#features" className="hover:text-white transition-colors">Возможности</a>
            <a href="#plans"    className="hover:text-white transition-colors">Тарифы</a>
            <a href="#downloads" className="hover:text-white transition-colors">Скачать</a>
            <a href="#faq"      className="hover:text-white transition-colors">FAQ</a>
          </div>
          <div className="flex items-center gap-3">
            <Link href="/auth/login"    className="btn btn-secondary text-sm">Войти</Link>
            <Link href="/auth/register" className="btn btn-primary text-sm glow-green">Начать</Link>
          </div>
        </div>
      </nav>

      {/* ── Hero ────────────────────────────────────────────────────────── */}
      <section className="relative pt-32 pb-24 px-4 overflow-hidden">
        {/* Background orbs */}
        <div className="absolute inset-0 pointer-events-none">
          <div className="absolute top-20 left-1/4 w-[500px] h-[500px] rounded-full blur-3xl opacity-8"
            style={{ background: 'radial-gradient(circle,#00ff88,transparent)' }} />
          <div className="absolute bottom-0 right-1/4 w-[400px] h-[400px] rounded-full blur-3xl opacity-6"
            style={{ background: 'radial-gradient(circle,#0066ff,transparent)' }} />
        </div>

        <div className="max-w-4xl mx-auto text-center relative">
          <motion.div initial="hidden" animate="visible" variants={stagger}>
            <motion.div variants={fadeUp}
              className="inline-flex items-center gap-2 px-4 py-2 rounded-full text-sm font-medium mb-6"
              style={{ background: 'rgba(0,255,136,0.1)', border: '1px solid rgba(0,255,136,0.3)', color: '#00ff88' }}>
              <Wifi className="w-4 h-4" />
              QUIC · Hysteria2 · AES-256
            </motion.div>

            <motion.h1 variants={fadeUp}
              className="text-5xl md:text-7xl font-black mb-6 leading-tight">
              VPN нового<br />
              <span className="gradient-text">поколения</span>
            </motion.h1>

            <motion.p variants={fadeUp}
              className="text-xl md:text-2xl mb-10 max-w-2xl mx-auto"
              style={{ color: 'var(--muted-foreground)' }}>
              Молниеносный, невидимый для цензуры VPN с оплатой через СБП.
              Никаких подписок автоматических — только когда нужно.
            </motion.p>

            <motion.div variants={fadeUp} className="flex flex-col sm:flex-row gap-4 justify-center">
              <Link href="/auth/register"
                className="btn btn-primary px-8 py-4 text-lg glow-green flex items-center gap-2">
                Попробовать бесплатно
                <ArrowRight className="w-5 h-5" />
              </Link>
              <a href="#downloads"
                className="btn btn-secondary px-8 py-4 text-lg flex items-center gap-2">
                <Download className="w-5 h-5" />
                Скачать приложение
              </a>
            </motion.div>

            {/* Stats row */}
            <motion.div variants={fadeUp}
              className="mt-16 grid grid-cols-3 gap-6 max-w-lg mx-auto">
              {[
                { val: '500+',  label: 'Мбит/с' },
                { val: '0',     label: 'Логов' },
                { val: '25%',   label: 'Реферальных' },
              ].map(s => (
                <div key={s.label} className="text-center">
                  <div className="text-3xl font-black gradient-text">{s.val}</div>
                  <div className="text-sm mt-1" style={{ color: 'var(--muted-foreground)' }}>{s.label}</div>
                </div>
              ))}
            </motion.div>
          </motion.div>
        </div>
      </section>

      {/* ── Features ────────────────────────────────────────────────────── */}
      <section id="features" className="py-24 px-4">
        <div className="max-w-6xl mx-auto">
          <motion.div initial="hidden" whileInView="visible" viewport={{ once: true }} variants={stagger}>
            <motion.div variants={fadeUp} className="text-center mb-16">
              <h2 className="text-4xl font-black mb-4">Почему Lowkey?</h2>
              <p className="text-lg" style={{ color: 'var(--muted-foreground)' }}>
                Технологии следующего поколения в простом приложении
              </p>
            </motion.div>
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
              {FEATURES.map(f => (
                <motion.div key={f.title} variants={fadeUp}
                  className="glass rounded-2xl p-6 card-hover">
                  <div className="w-12 h-12 rounded-xl mb-4 flex items-center justify-center"
                    style={{ background: 'rgba(0,255,136,0.1)' }}>
                    <f.icon className="w-6 h-6" style={{ color: '#00ff88' }} />
                  </div>
                  <h3 className="font-bold text-lg mb-2">{f.title}</h3>
                  <p className="text-sm leading-relaxed" style={{ color: 'var(--muted-foreground)' }}>{f.desc}</p>
                </motion.div>
              ))}
            </div>
          </motion.div>
        </div>
      </section>

      {/* ── How it works ────────────────────────────────────────────────── */}
      <section className="py-24 px-4" style={{ background: 'rgba(255,255,255,0.02)' }}>
        <div className="max-w-5xl mx-auto">
          <motion.div initial="hidden" whileInView="visible" viewport={{ once: true }} variants={stagger}>
            <motion.div variants={fadeUp} className="text-center mb-16">
              <h2 className="text-4xl font-black mb-4">Как это работает</h2>
              <p style={{ color: 'var(--muted-foreground)' }}>Четыре шага до защищённого соединения</p>
            </motion.div>
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-8">
              {HOW_IT_WORKS.map((step, i) => (
                <motion.div key={step.num} variants={fadeUp} className="text-center">
                  <div className="text-5xl font-black mb-4 gradient-text opacity-30">{step.num}</div>
                  <div className="w-12 h-12 rounded-full mx-auto mb-4 flex items-center justify-center"
                    style={{ background: 'rgba(0,255,136,0.1)', border: '2px solid rgba(0,255,136,0.3)' }}>
                    {i === 0 && <Download className="w-5 h-5" style={{ color: '#00ff88' }} />}
                    {i === 1 && <Users    className="w-5 h-5" style={{ color: '#00ff88' }} />}
                    {i === 2 && <Award    className="w-5 h-5" style={{ color: '#00ff88' }} />}
                    {i === 3 && <Wifi     className="w-5 h-5" style={{ color: '#00ff88' }} />}
                  </div>
                  <h3 className="font-bold mb-2">{step.title}</h3>
                  <p className="text-sm" style={{ color: 'var(--muted-foreground)' }}>{step.desc}</p>
                </motion.div>
              ))}
            </div>
          </motion.div>
        </div>
      </section>

      {/* ── Plans ───────────────────────────────────────────────────────── */}
      <section id="plans" className="py-24 px-4">
        <div className="max-w-5xl mx-auto">
          <motion.div initial="hidden" whileInView="visible" viewport={{ once: true }} variants={stagger}>
            <motion.div variants={fadeUp} className="text-center mb-16">
              <h2 className="text-4xl font-black mb-4">Тарифные планы</h2>
              <p style={{ color: 'var(--muted-foreground)' }}>
                Оплата через СБП — мгновенное зачисление. Никаких автопродлений.
              </p>
            </motion.div>

            {plans.length === 0 ? (
              // Fallback static plans while loading
              <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
                {[
                  { name: 'Базовый', price: 199, speed: '10 Мбит/с', days: 30, popular: false },
                  { name: 'Стандарт', price: 299, speed: '50 Мбит/с', days: 30, popular: true },
                  { name: 'Премиум', price: 499, speed: '∞ Мбит/с', days: 30, popular: false },
                ].map(p => (
                  <PlanCard key={p.name} name={p.name} price={p.price} speed={p.speed}
                    days={p.days} popular={p.popular} />
                ))}
              </div>
            ) : (
              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
                {plans.filter(p => !p.is_bundle).map((p, i) => (
                  <motion.div key={p.plan_key || i} variants={fadeUp}>
                    <PlanCard
                      name={p.name}
                      price={p.price_rub}
                      speed={p.speed_mbps === 0 ? '∞ Мбит/с' : `${p.speed_mbps} Мбит/с`}
                      days={p.duration_days}
                      popular={p.name.toLowerCase().includes('стандарт')}
                    />
                  </motion.div>
                ))}
              </div>
            )}

            {/* Bundle plans */}
            {plans.filter(p => p.is_bundle).length > 0 && (
              <motion.div variants={fadeUp} className="mt-8">
                <p className="text-center text-sm mb-4" style={{ color: 'var(--muted-foreground)' }}>
                  Абонементы со скидкой
                </p>
                <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                  {plans.filter(p => p.is_bundle).map((p, i) => (
                    <Link key={p.plan_key || i} href="/auth/register"
                      className="glass rounded-xl p-4 flex items-center justify-between card-hover">
                      <div>
                        <div className="font-semibold text-sm">{p.name}</div>
                        <div className="text-xs mt-0.5" style={{ color: 'var(--muted-foreground)' }}>
                          {p.duration_days} дней
                        </div>
                      </div>
                      <div className="text-right">
                        <div className="font-bold gradient-text">{p.price_rub.toFixed(0)} ₽</div>
                        {p.discount_pct && p.discount_pct > 0 && (
                          <div className="text-xs" style={{ color: '#00ff88' }}>−{p.discount_pct}%</div>
                        )}
                      </div>
                    </Link>
                  ))}
                </div>
              </motion.div>
            )}
          </motion.div>
        </div>
      </section>

      {/* ── Downloads ───────────────────────────────────────────────────── */}
      <section id="downloads" className="py-24 px-4" style={{ background: 'rgba(255,255,255,0.02)' }}>
        <div className="max-w-4xl mx-auto">
          <motion.div initial="hidden" whileInView="visible" viewport={{ once: true }} variants={stagger}>
            <motion.div variants={fadeUp} className="text-center mb-12">
              <h2 className="text-4xl font-black mb-4">Скачать приложение</h2>
              <p style={{ color: 'var(--muted-foreground)' }}>Доступно для Windows, Linux и Android</p>
            </motion.div>

            <motion.div variants={stagger} className="grid grid-cols-1 md:grid-cols-3 gap-6">
              {[
                { platform: 'windows', icon: <Monitor className="w-8 h-8" />, label: 'Windows', sub: 'Windows 10/11 (x64)' },
                { platform: 'linux',   icon: <Server   className="w-8 h-8" />, label: 'Linux',   sub: 'Ubuntu / Debian / Fedora (x64)' },
                { platform: 'android', icon: <Smartphone className="w-8 h-8" />, label: 'Android', sub: 'Android 8.0+' },
              ].map(({ platform, icon, label, sub }) => {
                const r = releases.find(x => x.platform === platform);
                return (
                  <motion.a key={platform} variants={fadeUp}
                    href={r?.download_url || `/downloads`}
                    className="glass rounded-2xl p-6 card-hover flex flex-col items-center text-center gap-4">
                    <div className="w-16 h-16 rounded-2xl flex items-center justify-center"
                      style={{ background: 'rgba(0,255,136,0.1)', color: '#00ff88' }}>
                      {icon}
                    </div>
                    <div>
                      <div className="font-bold text-lg">{label}</div>
                      <div className="text-sm" style={{ color: 'var(--muted-foreground)' }}>{sub}</div>
                      {r && (
                        <div className="text-xs mt-1" style={{ color: '#00ff88' }}>v{r.version}</div>
                      )}
                    </div>
                    <div className="btn btn-primary w-full flex items-center justify-center gap-2 mt-auto">
                      <Download className="w-4 h-4" />
                      {r ? 'Скачать' : 'Скоро'}
                    </div>
                  </motion.a>
                );
              })}
            </motion.div>

            {/* Quick install */}
            <motion.div variants={fadeUp} className="mt-10 glass rounded-2xl p-6">
              <h3 className="font-bold mb-4">Быстрая установка</h3>
              <div className="space-y-4">
                <div>
                  <div className="text-sm font-medium mb-2" style={{ color: '#00ff88' }}>Linux (CLI)</div>
                  <code className="block p-3 rounded-xl text-sm font-mono overflow-x-auto"
                    style={{ background: 'rgba(0,0,0,0.4)', color: 'var(--muted-foreground)' }}>
                    curl -fsSL https://get.lowkeyvpn.com/linux | sudo bash
                  </code>
                </div>
                <div>
                  <div className="text-sm font-medium mb-2" style={{ color: '#00ff88' }}>Windows (PowerShell)</div>
                  <code className="block p-3 rounded-xl text-sm font-mono overflow-x-auto"
                    style={{ background: 'rgba(0,0,0,0.4)', color: 'var(--muted-foreground)' }}>
                    irm https://get.lowkeyvpn.com/windows | iex
                  </code>
                </div>
              </div>
            </motion.div>
          </motion.div>
        </div>
      </section>

      {/* ── FAQ ─────────────────────────────────────────────────────────── */}
      <section id="faq" className="py-24 px-4">
        <div className="max-w-3xl mx-auto">
          <motion.div initial="hidden" whileInView="visible" viewport={{ once: true }} variants={stagger}>
            <motion.div variants={fadeUp} className="text-center mb-12">
              <h2 className="text-4xl font-black mb-4">Часто задаваемые вопросы</h2>
            </motion.div>
            <div className="space-y-3">
              {FAQ.map((item, i) => (
                <motion.div key={i} variants={fadeUp}
                  className="glass rounded-2xl overflow-hidden">
                  <button
                    onClick={() => setOpenFaq(openFaq === i ? null : i)}
                    className="w-full flex items-center justify-between p-5 text-left">
                    <span className="font-semibold">{item.q}</span>
                    <ChevronDown
                      className="w-5 h-5 flex-shrink-0 transition-transform"
                      style={{
                        color: 'var(--muted-foreground)',
                        transform: openFaq === i ? 'rotate(180deg)' : 'none',
                      }}
                    />
                  </button>
                  {openFaq === i && (
                    <div className="px-5 pb-5 text-sm leading-relaxed"
                      style={{ color: 'var(--muted-foreground)' }}>
                      {item.a}
                    </div>
                  )}
                </motion.div>
              ))}
            </div>
          </motion.div>
        </div>
      </section>

      {/* ── CTA ─────────────────────────────────────────────────────────── */}
      <section className="py-24 px-4">
        <div className="max-w-2xl mx-auto text-center">
          <motion.div initial="hidden" whileInView="visible" viewport={{ once: true }} variants={stagger}>
            <motion.h2 variants={fadeUp} className="text-4xl font-black mb-4">
              Начните прямо сейчас
            </motion.h2>
            <motion.p variants={fadeUp} className="text-lg mb-8" style={{ color: 'var(--muted-foreground)' }}>
              Регистрация за 30 секунд. Оплата через СБП. Никаких карт.
            </motion.p>
            <motion.div variants={fadeUp} className="flex flex-col sm:flex-row gap-4 justify-center">
              <Link href="/auth/register" className="btn btn-primary px-8 py-4 text-lg glow-green">
                Создать аккаунт бесплатно
              </Link>
              <Link href="/downloads" className="btn btn-secondary px-8 py-4 text-lg">
                Скачать приложение
              </Link>
            </motion.div>
          </motion.div>
        </div>
      </section>

      {/* ── Footer ──────────────────────────────────────────────────────── */}
      <footer className="border-t py-10 px-4" style={{ borderColor: 'var(--border)' }}>
        <div className="max-w-6xl mx-auto flex flex-col md:flex-row items-center justify-between gap-4">
          <div className="flex items-center gap-2">
            <div className="w-6 h-6 rounded flex items-center justify-center"
              style={{ background: 'linear-gradient(135deg,#00ff88,#0066ff)' }}>
              <Shield className="w-3 h-3 text-black" />
            </div>
            <span className="font-bold gradient-text">Lowkey VPN</span>
          </div>
          <div className="flex items-center gap-6 text-sm" style={{ color: 'var(--muted-foreground)' }}>
            <Link href="/downloads"    className="hover:text-white transition-colors">Скачать</Link>
            <Link href="/auth/login"   className="hover:text-white transition-colors">Войти</Link>
            <Link href="/auth/register" className="hover:text-white transition-colors">Регистрация</Link>
          </div>
          <p className="text-xs" style={{ color: 'var(--muted-foreground)' }}>
            © {new Date().getFullYear()} Lowkey VPN. Все права защищены.
          </p>
        </div>
      </footer>
    </div>
  );
}

function PlanCard({ name, price, speed, days, popular }: {
  name: string; price: number; speed: string; days: number; popular: boolean;
}) {
  const perks = [`${speed}`, `${days} дней`, 'СБП оплата', 'Без логов', 'Все протоколы'];
  return (
    <div className={`glass rounded-2xl p-6 flex flex-col relative ${popular ? 'ring-2 ring-green-400/40' : ''}`}>
      {popular && (
        <div className="absolute -top-3 left-1/2 -translate-x-1/2 px-3 py-1 rounded-full text-xs font-bold text-black"
          style={{ background: 'linear-gradient(135deg,#00ff88,#0066ff)' }}>
          Популярный
        </div>
      )}
      <div className="mb-6">
        <h3 className="text-xl font-bold mb-1">{name}</h3>
        <div className="flex items-baseline gap-1">
          <span className="text-4xl font-black gradient-text">{price}</span>
          <span className="text-lg" style={{ color: 'var(--muted-foreground)' }}>₽</span>
          <span className="text-sm" style={{ color: 'var(--muted-foreground)' }}>/мес</span>
        </div>
      </div>
      <ul className="space-y-2.5 mb-8 flex-1">
        {perks.map(p => (
          <li key={p} className="flex items-center gap-2 text-sm">
            <Check className="w-4 h-4 flex-shrink-0" style={{ color: '#00ff88' }} />
            <span>{p}</span>
          </li>
        ))}
      </ul>
      <Link href="/auth/register" className={`btn text-center py-3 ${popular ? 'btn-primary glow-green' : 'btn-secondary'}`}>
        Начать
      </Link>
    </div>
  );
}
