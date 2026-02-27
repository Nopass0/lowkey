import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { motion } from 'framer-motion';
import { Shield, LogIn } from 'lucide-react';
import { useStore } from '../store';

export default function LoginView() {
  const { setToken, setUser, apiUrl, setApiUrl } = useStore();
  const [login, setLogin] = useState('');
  const [password, setPassword] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [showSettings, setShowSettings] = useState(false);
  const [apiUrlInput, setApiUrlInput] = useState(apiUrl);

  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setError('');
    try {
      const res = await invoke<any>('api_login', { apiUrl, login, password });
      setToken(res.token);
      setUser(res.user);
    } catch (err: any) {
      setError(typeof err === 'string' ? err : 'Неверный логин или пароль');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="h-screen flex flex-col items-center justify-center p-6" style={{ background: 'var(--bg)' }}>
      {/* Background glow */}
      <div className="fixed inset-0 pointer-events-none">
        <div className="absolute top-1/4 left-1/2 -translate-x-1/2 w-64 h-64 rounded-full opacity-10 blur-3xl"
          style={{ background: 'radial-gradient(circle, #00ff88, transparent)' }} />
      </div>

      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        className="w-full"
      >
        {/* Logo */}
        <div className="text-center mb-8">
          <div className="w-16 h-16 rounded-2xl mx-auto mb-4 flex items-center justify-center"
            style={{ background: 'linear-gradient(135deg, #00ff88, #0066ff)' }}>
            <Shield className="w-8 h-8 text-black" />
          </div>
          <h1 className="text-2xl font-bold gradient-text">Lowkey VPN</h1>
          <p className="text-sm mt-1" style={{ color: 'var(--muted)' }}>Безопасный и быстрый VPN</p>
        </div>

        {error && (
          <motion.div
            initial={{ opacity: 0 }} animate={{ opacity: 1 }}
            className="rounded-xl p-3 mb-4 text-sm text-center"
            style={{ background: 'rgba(255,68,68,0.1)', color: 'var(--danger)', border: '1px solid rgba(255,68,68,0.2)' }}
          >
            {error}
          </motion.div>
        )}

        <form onSubmit={handleLogin} className="space-y-3">
          <input
            type="text"
            value={login}
            onChange={e => setLogin(e.target.value)}
            placeholder="Логин"
            required
            autoComplete="username"
          />
          <input
            type="password"
            value={password}
            onChange={e => setPassword(e.target.value)}
            placeholder="Пароль"
            required
            autoComplete="current-password"
          />
          <button type="submit" disabled={loading}
            className="btn btn-primary w-full py-3 mt-2">
            {loading ? (
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

        <button
          onClick={() => setShowSettings(!showSettings)}
          className="text-xs mt-4 w-full text-center"
          style={{ color: 'var(--muted)' }}
        >
          Настройки сервера
        </button>

        {showSettings && (
          <motion.div initial={{ opacity: 0, height: 0 }} animate={{ opacity: 1, height: 'auto' }}
            className="mt-3 space-y-2">
            <input
              type="text"
              value={apiUrlInput}
              onChange={e => setApiUrlInput(e.target.value)}
              placeholder="URL API сервера"
            />
            <button
              onClick={() => { setApiUrl(apiUrlInput); setShowSettings(false); }}
              className="btn btn-secondary w-full text-sm"
            >
              Сохранить
            </button>
          </motion.div>
        )}
      </motion.div>
    </div>
  );
}
