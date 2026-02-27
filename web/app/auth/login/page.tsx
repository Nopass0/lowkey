'use client';

import { useState } from 'react';
import { motion } from 'framer-motion';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { Shield, LogIn, Eye, EyeOff } from 'lucide-react';
import { useAuthStore } from '@/store/auth';

export default function LoginPage() {
  const router = useRouter();
  const { login, isLoading } = useAuthStore();
  const [login_, setLogin] = useState('');
  const [password, setPassword] = useState('');
  const [showPw, setShowPw] = useState(false);
  const [error, setError] = useState('');

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    try {
      await login(login_, password);
      router.push('/dashboard');
    } catch (err: any) {
      setError(err.message || 'Неверный логин или пароль');
    }
  };

  return (
    <div className="min-h-screen flex items-center justify-center px-4" style={{ background: 'var(--background)' }}>
      {/* Background orbs */}
      <div className="fixed inset-0 overflow-hidden pointer-events-none">
        <div className="absolute top-1/4 left-1/3 w-96 h-96 rounded-full opacity-5 blur-3xl"
          style={{ background: 'radial-gradient(circle, #00ff88, transparent)' }} />
        <div className="absolute bottom-1/4 right-1/3 w-96 h-96 rounded-full opacity-5 blur-3xl"
          style={{ background: 'radial-gradient(circle, #0066ff, transparent)' }} />
      </div>

      <motion.div
        initial={{ opacity: 0, y: 40 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.5 }}
        className="w-full max-w-md"
      >
        <div className="glass rounded-3xl p-8">
          {/* Logo */}
          <div className="text-center mb-8">
            <Link href="/" className="inline-flex items-center gap-2 mb-4">
              <div className="w-10 h-10 rounded-xl glow-green flex items-center justify-center"
                style={{ background: 'linear-gradient(135deg, #00ff88, #0066ff)' }}>
                <Shield className="w-5 h-5 text-black" />
              </div>
              <span className="text-xl font-bold gradient-text">Lowkey VPN</span>
            </Link>
            <h1 className="text-2xl font-bold">Вход в аккаунт</h1>
            <p className="text-sm mt-2" style={{ color: 'var(--muted-foreground)' }}>
              Нет аккаунта? <Link href="/auth/register" className="hover:text-white transition-colors" style={{ color: '#00ff88' }}>Зарегистрироваться</Link>
            </p>
          </div>

          {error && (
            <motion.div
              initial={{ opacity: 0, scale: 0.95 }}
              animate={{ opacity: 1, scale: 1 }}
              className="rounded-xl p-4 mb-6 text-sm"
              style={{ background: 'rgba(255,68,68,0.1)', border: '1px solid rgba(255,68,68,0.3)', color: '#ff4444' }}
            >
              {error}
            </motion.div>
          )}

          <form onSubmit={handleSubmit} className="space-y-4">
            <div>
              <label className="block text-sm font-medium mb-1.5" style={{ color: 'var(--muted-foreground)' }}>
                Логин
              </label>
              <input
                type="text"
                value={login_}
                onChange={e => setLogin(e.target.value)}
                placeholder="Введите логин"
                required
                className="w-full"
              />
            </div>

            <div>
              <label className="block text-sm font-medium mb-1.5" style={{ color: 'var(--muted-foreground)' }}>
                Пароль
              </label>
              <div className="relative">
                <input
                  type={showPw ? 'text' : 'password'}
                  value={password}
                  onChange={e => setPassword(e.target.value)}
                  placeholder="Введите пароль"
                  required
                  className="w-full pr-10"
                />
                <button
                  type="button"
                  onClick={() => setShowPw(!showPw)}
                  className="absolute right-3 top-1/2 -translate-y-1/2"
                  style={{ color: 'var(--muted-foreground)' }}
                >
                  {showPw ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
                </button>
              </div>
            </div>

            <button
              type="submit"
              disabled={isLoading}
              className="btn btn-primary w-full py-3 mt-2 glow-green"
            >
              {isLoading ? (
                <span className="flex items-center gap-2">
                  <div className="w-4 h-4 border-2 border-black/30 border-t-black rounded-full animate-spin" />
                  Вход...
                </span>
              ) : (
                <span className="flex items-center gap-2">
                  <LogIn className="w-4 h-4" />
                  Войти
                </span>
              )}
            </button>
          </form>
        </div>
      </motion.div>
    </div>
  );
}
