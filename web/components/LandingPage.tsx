'use client';

import { motion } from 'framer-motion';
import Link from 'next/link';
import { Shield, Zap, Globe, Lock, Star, Download, ChevronRight, Check } from 'lucide-react';

const fadeInUp = {
  hidden: { opacity: 0, y: 40 },
  visible: { opacity: 1, y: 0 },
};

const stagger = {
  visible: { transition: { staggerChildren: 0.15 } },
};

const features = [
  { icon: Shield, title: 'Полная защита', desc: 'Шифрование AES-256 и X25519 для всего вашего трафика' },
  { icon: Zap, title: 'Высокая скорость', desc: 'До 500+ Мбит/с без ограничений на премиум-тарифе' },
  { icon: Globe, title: 'Обход блокировок', desc: 'Доступ к любым сайтам и сервисам без ограничений' },
  { icon: Lock, title: 'Нет логов', desc: 'Мы не храним данные о вашей активности и не передаём третьим лицам' },
];

const plans = [
  { name: 'Базовый', price: 199, speed: '10 Мбит/с', features: ['1 устройство', '10 Мбит/с', '30 дней', 'SBP оплата'], popular: false },
  { name: 'Стандарт', price: 299, speed: '50 Мбит/с', features: ['3 устройства', '50 Мбит/с', '30 дней', 'SBP оплата', 'Приоритетная поддержка'], popular: true },
  { name: 'Премиум', price: 499, speed: 'Без лимита', features: ['Неограничено устройств', 'Без ограничений', '30 дней', 'SBP оплата', 'VIP поддержка', 'Выделенный IP'], popular: false },
];

const downloads = [
  { platform: 'Windows', icon: '🪟', href: '/downloads/windows' },
  { platform: 'Linux', icon: '🐧', href: '/downloads/linux' },
  { platform: 'Android', icon: '🤖', href: '/downloads/android' },
];

export default function LandingPage() {
  return (
    <div className="min-h-screen" style={{ background: 'var(--background)' }}>
      {/* Navigation */}
      <nav className="fixed top-0 inset-x-0 z-50 glass border-b border-white/5">
        <div className="max-w-6xl mx-auto px-4 h-16 flex items-center justify-between">
          <motion.div
            initial={{ opacity: 0, x: -20 }}
            animate={{ opacity: 1, x: 0 }}
            className="flex items-center gap-2"
          >
            <div className="w-8 h-8 rounded-lg glow-green flex items-center justify-center"
              style={{ background: 'linear-gradient(135deg, #00ff88, #0066ff)' }}>
              <Shield className="w-4 h-4 text-black" />
            </div>
            <span className="text-lg font-bold gradient-text">Lowkey VPN</span>
          </motion.div>

          <div className="hidden md:flex items-center gap-8 text-sm" style={{ color: 'var(--muted-foreground)' }}>
            <a href="#features" className="hover:text-white transition-colors">Функции</a>
            <a href="#plans" className="hover:text-white transition-colors">Тарифы</a>
            <a href="#downloads" className="hover:text-white transition-colors">Скачать</a>
            <a href="#referral" className="hover:text-white transition-colors">Реферальная программа</a>
          </div>

          <div className="flex items-center gap-3">
            <Link href="/auth/login">
              <button className="btn btn-secondary text-sm">Войти</button>
            </Link>
            <Link href="/auth/register">
              <button className="btn btn-primary text-sm glow-green">Начать</button>
            </Link>
          </div>
        </div>
      </nav>

      {/* Hero */}
      <section className="pt-32 pb-24 px-4 text-center relative overflow-hidden">
        {/* Background orbs */}
        <div className="absolute top-20 left-1/4 w-96 h-96 rounded-full opacity-10 blur-3xl pointer-events-none"
          style={{ background: 'radial-gradient(circle, #00ff88, transparent)' }} />
        <div className="absolute top-40 right-1/4 w-96 h-96 rounded-full opacity-10 blur-3xl pointer-events-none"
          style={{ background: 'radial-gradient(circle, #0066ff, transparent)' }} />

        <motion.div
          variants={stagger}
          initial="hidden"
          animate="visible"
          className="max-w-4xl mx-auto"
        >
          <motion.div variants={fadeInUp}
            className="inline-flex items-center gap-2 px-4 py-2 rounded-full text-sm mb-6 glass"
            style={{ color: '#00ff88', border: '1px solid rgba(0,255,136,0.2)' }}>
            <Star className="w-3.5 h-3.5" />
            <span>Реферальная программа — зарабатывайте 25% с каждого платежа</span>
          </motion.div>

          <motion.h1 variants={fadeInUp}
            className="text-5xl md:text-7xl font-bold mb-6 leading-tight">
            Ваш{' '}
            <span className="gradient-text">безопасный</span>
            <br />
            интернет без границ
          </motion.h1>

          <motion.p variants={fadeInUp}
            className="text-xl mb-10 max-w-2xl mx-auto"
            style={{ color: 'var(--muted-foreground)' }}>
            Высокоскоростной VPN с оплатой через СБП, реферальной программой и поддержкой
            всех популярных устройств.
          </motion.p>

          <motion.div variants={fadeInUp} className="flex flex-col sm:flex-row gap-4 justify-center">
            <Link href="/auth/register">
              <button className="btn btn-primary px-8 py-4 text-base glow-green">
                Попробовать бесплатно
                <ChevronRight className="w-4 h-4" />
              </button>
            </Link>
            <a href="#plans">
              <button className="btn btn-secondary px-8 py-4 text-base">
                Посмотреть тарифы
              </button>
            </a>
          </motion.div>

          <motion.div variants={fadeInUp}
            className="mt-12 grid grid-cols-3 gap-8 max-w-xl mx-auto text-center">
            {[
              { value: '10К+', label: 'Пользователей' },
              { value: '99.9%', label: 'Uptime' },
              { value: '<1мс', label: 'Задержка' },
            ].map((stat) => (
              <div key={stat.label}>
                <div className="text-3xl font-bold gradient-text">{stat.value}</div>
                <div className="text-sm mt-1" style={{ color: 'var(--muted-foreground)' }}>{stat.label}</div>
              </div>
            ))}
          </motion.div>
        </motion.div>
      </section>

      {/* Features */}
      <section id="features" className="py-24 px-4">
        <div className="max-w-6xl mx-auto">
          <motion.div
            variants={stagger}
            initial="hidden"
            whileInView="visible"
            viewport={{ once: true }}
            className="text-center mb-16"
          >
            <motion.h2 variants={fadeInUp} className="text-4xl font-bold mb-4">
              Почему <span className="gradient-text">Lowkey VPN</span>?
            </motion.h2>
            <motion.p variants={fadeInUp} style={{ color: 'var(--muted-foreground)' }}>
              Мы создали VPN для тех, кто ценит скорость, безопасность и удобство
            </motion.p>
          </motion.div>

          <motion.div
            variants={stagger}
            initial="hidden"
            whileInView="visible"
            viewport={{ once: true }}
            className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6"
          >
            {features.map((f) => (
              <motion.div key={f.title} variants={fadeInUp}
                className="glass rounded-2xl p-6 card-hover text-center">
                <div className="w-12 h-12 rounded-xl mx-auto mb-4 flex items-center justify-center"
                  style={{ background: 'linear-gradient(135deg, rgba(0,255,136,0.2), rgba(0,102,255,0.2))' }}>
                  <f.icon className="w-6 h-6" style={{ color: '#00ff88' }} />
                </div>
                <h3 className="text-lg font-semibold mb-2">{f.title}</h3>
                <p className="text-sm" style={{ color: 'var(--muted-foreground)' }}>{f.desc}</p>
              </motion.div>
            ))}
          </motion.div>
        </div>
      </section>

      {/* Plans */}
      <section id="plans" className="py-24 px-4">
        <div className="max-w-6xl mx-auto">
          <motion.div
            initial="hidden" whileInView="visible" viewport={{ once: true }}
            variants={stagger} className="text-center mb-16"
          >
            <motion.h2 variants={fadeInUp} className="text-4xl font-bold mb-4">
              Гибкие <span className="gradient-text">тарифы</span>
            </motion.h2>
            <motion.p variants={fadeInUp} style={{ color: 'var(--muted-foreground)' }}>
              Оплата через СБП — мгновенно и без комиссий. Скидка 50% при регистрации по реферальной ссылке.
            </motion.p>
          </motion.div>

          <motion.div
            variants={stagger} initial="hidden" whileInView="visible" viewport={{ once: true }}
            className="grid grid-cols-1 md:grid-cols-3 gap-6"
          >
            {plans.map((plan) => (
              <motion.div key={plan.name} variants={fadeInUp}
                className={`glass rounded-2xl p-8 card-hover relative ${plan.popular ? 'ring-1 ring-green-400/50' : ''}`}>
                {plan.popular && (
                  <div className="absolute -top-3 left-1/2 -translate-x-1/2">
                    <span className="px-4 py-1 rounded-full text-xs font-semibold text-black"
                      style={{ background: '#00ff88' }}>
                      Популярный
                    </span>
                  </div>
                )}
                <h3 className="text-xl font-bold mb-2">{plan.name}</h3>
                <div className="mb-1">
                  <span className="text-4xl font-bold gradient-text">{plan.price}</span>
                  <span className="text-lg" style={{ color: 'var(--muted-foreground)' }}>₽/мес</span>
                </div>
                <p className="text-sm mb-6" style={{ color: 'var(--muted-foreground)' }}>{plan.speed}</p>

                <ul className="space-y-3 mb-8">
                  {plan.features.map((f) => (
                    <li key={f} className="flex items-center gap-2 text-sm">
                      <Check className="w-4 h-4 flex-shrink-0" style={{ color: '#00ff88' }} />
                      {f}
                    </li>
                  ))}
                </ul>

                <Link href="/auth/register">
                  <button className={`btn w-full ${plan.popular ? 'btn-primary' : 'btn-secondary'}`}>
                    Выбрать тариф
                  </button>
                </Link>
              </motion.div>
            ))}
          </motion.div>
        </div>
      </section>

      {/* Referral */}
      <section id="referral" className="py-24 px-4">
        <div className="max-w-4xl mx-auto">
          <motion.div
            initial="hidden" whileInView="visible" viewport={{ once: true }}
            variants={stagger}
            className="glass rounded-3xl p-12 text-center"
            style={{ border: '1px solid rgba(0,255,136,0.2)' }}
          >
            <motion.div variants={fadeInUp}
              className="w-16 h-16 rounded-2xl mx-auto mb-6 flex items-center justify-center"
              style={{ background: 'linear-gradient(135deg, rgba(0,255,136,0.2), rgba(0,102,255,0.2))' }}>
              <Star className="w-8 h-8" style={{ color: '#00ff88' }} />
            </motion.div>
            <motion.h2 variants={fadeInUp} className="text-4xl font-bold mb-4">
              Реферальная <span className="gradient-text">программа</span>
            </motion.h2>
            <motion.p variants={fadeInUp} className="text-lg mb-8" style={{ color: 'var(--muted-foreground)' }}>
              Приглашайте друзей и зарабатывайте <strong className="text-white">25%</strong> с каждого их платежа на свой реферальный счёт.
              Ваш друг получит <strong className="text-white">скидку 50%</strong> на первую подписку.
              Выводите средства на карту через СБП.
            </motion.p>
            <motion.div variants={fadeInUp} className="grid grid-cols-3 gap-6 mb-10">
              {[
                { val: '25%', label: 'Комиссия с платежей' },
                { val: '50%', label: 'Скидка другу' },
                { val: '∞', label: 'Уровни рефералов' },
              ].map((s) => (
                <div key={s.label}>
                  <div className="text-3xl font-bold gradient-text">{s.val}</div>
                  <div className="text-sm mt-1" style={{ color: 'var(--muted-foreground)' }}>{s.label}</div>
                </div>
              ))}
            </motion.div>
            <motion.div variants={fadeInUp}>
              <Link href="/auth/register">
                <button className="btn btn-primary px-10 py-4 glow-green">
                  Начать зарабатывать
                </button>
              </Link>
            </motion.div>
          </motion.div>
        </div>
      </section>

      {/* Downloads */}
      <section id="downloads" className="py-24 px-4">
        <div className="max-w-4xl mx-auto text-center">
          <motion.div
            initial="hidden" whileInView="visible" viewport={{ once: true }}
            variants={stagger}
          >
            <motion.h2 variants={fadeInUp} className="text-4xl font-bold mb-4">
              Скачать <span className="gradient-text">приложение</span>
            </motion.h2>
            <motion.p variants={fadeInUp} className="mb-12" style={{ color: 'var(--muted-foreground)' }}>
              Доступно для всех популярных платформ
            </motion.p>
            <motion.div variants={stagger} className="flex flex-wrap justify-center gap-4">
              {downloads.map((d) => (
                <motion.a key={d.platform} variants={fadeInUp} href={d.href}
                  className="flex items-center gap-3 glass rounded-2xl px-8 py-4 card-hover">
                  <span className="text-3xl">{d.icon}</span>
                  <div className="text-left">
                    <div className="text-xs" style={{ color: 'var(--muted-foreground)' }}>Скачать для</div>
                    <div className="font-semibold">{d.platform}</div>
                  </div>
                  <Download className="w-4 h-4 ml-2" style={{ color: '#00ff88' }} />
                </motion.a>
              ))}
            </motion.div>
          </motion.div>
        </div>
      </section>

      {/* CTA */}
      <section className="py-24 px-4">
        <div className="max-w-3xl mx-auto text-center">
          <motion.div
            initial="hidden" whileInView="visible" viewport={{ once: true }}
            variants={stagger}
          >
            <motion.h2 variants={fadeInUp} className="text-4xl font-bold mb-6">
              Готовы к <span className="gradient-text">приватному</span> интернету?
            </motion.h2>
            <motion.p variants={fadeInUp} className="mb-10 text-lg" style={{ color: 'var(--muted-foreground)' }}>
              Присоединяйтесь к тысячам пользователей Lowkey VPN сегодня
            </motion.p>
            <motion.div variants={fadeInUp}>
              <Link href="/auth/register">
                <button className="btn btn-primary px-12 py-5 text-lg glow-green">
                  Начать бесплатно
                  <ChevronRight className="w-5 h-5" />
                </button>
              </Link>
            </motion.div>
          </motion.div>
        </div>
      </section>

      {/* Footer */}
      <footer className="border-t py-8 px-4" style={{ borderColor: 'var(--border)' }}>
        <div className="max-w-6xl mx-auto flex flex-col md:flex-row items-center justify-between gap-4">
          <div className="flex items-center gap-2">
            <div className="w-6 h-6 rounded flex items-center justify-center"
              style={{ background: 'linear-gradient(135deg, #00ff88, #0066ff)' }}>
              <Shield className="w-3 h-3 text-black" />
            </div>
            <span className="font-bold gradient-text">Lowkey VPN</span>
          </div>
          <p className="text-sm" style={{ color: 'var(--muted-foreground)' }}>
            © 2025 Lowkey VPN. Все права защищены.
          </p>
          <div className="flex gap-6 text-sm" style={{ color: 'var(--muted-foreground)' }}>
            <Link href="/auth/login" className="hover:text-white transition-colors">Войти</Link>
            <Link href="/auth/register" className="hover:text-white transition-colors">Регистрация</Link>
            <Link href="/admin" className="hover:text-white transition-colors">Админ</Link>
          </div>
        </div>
      </footer>
    </div>
  );
}
