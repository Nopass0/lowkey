'use client';

import { motion } from 'framer-motion';
import Link from 'next/link';
import { Shield, Download, ExternalLink } from 'lucide-react';

const releases = [
  {
    platform: 'Windows',
    icon: '🪟',
    description: 'Windows 10/11 (x64)',
    filename: 'lowkey-vpn-windows-x64.exe',
    href: '/api/download/windows',
    version: '1.0.0',
  },
  {
    platform: 'Linux',
    icon: '🐧',
    description: 'Ubuntu/Debian/Fedora (x64)',
    filename: 'lowkey-vpn-linux-x64',
    href: '/api/download/linux',
    version: '1.0.0',
  },
  {
    platform: 'Android',
    icon: '🤖',
    description: 'Android 8.0+',
    filename: 'lowkey-vpn.apk',
    href: '/api/download/android',
    version: '1.0.0',
  },
];

export default function DownloadsPage() {
  return (
    <div className="min-h-screen px-4 py-16" style={{ background: 'var(--background)' }}>
      <div className="max-w-3xl mx-auto">
        <div className="flex items-center gap-2 mb-12">
          <Link href="/" className="flex items-center gap-2">
            <div className="w-8 h-8 rounded-lg flex items-center justify-center"
              style={{ background: 'linear-gradient(135deg, #00ff88, #0066ff)' }}>
              <Shield className="w-4 h-4 text-black" />
            </div>
            <span className="font-bold gradient-text">Lowkey VPN</span>
          </Link>
        </div>

        <motion.div initial={{ opacity: 0, y: 30 }} animate={{ opacity: 1, y: 0 }}>
          <h1 className="text-4xl font-bold mb-4">Скачать приложение</h1>
          <p className="mb-10" style={{ color: 'var(--muted-foreground)' }}>
            Выберите версию для вашей платформы
          </p>

          <div className="space-y-4">
            {releases.map((r, i) => (
              <motion.div key={r.platform}
                initial={{ opacity: 0, x: -20 }}
                animate={{ opacity: 1, x: 0 }}
                transition={{ delay: i * 0.1 }}
                className="glass rounded-2xl p-6 flex items-center justify-between card-hover">
                <div className="flex items-center gap-4">
                  <div className="text-4xl">{r.icon}</div>
                  <div>
                    <h3 className="font-semibold text-lg">{r.platform}</h3>
                    <p className="text-sm" style={{ color: 'var(--muted-foreground)' }}>{r.description}</p>
                    <p className="text-xs mt-1" style={{ color: 'var(--muted-foreground)' }}>
                      v{r.version} · {r.filename}
                    </p>
                  </div>
                </div>
                <a href={r.href} className="btn btn-primary flex items-center gap-2">
                  <Download className="w-4 h-4" />
                  Скачать
                </a>
              </motion.div>
            ))}
          </div>

          <div className="mt-12 glass rounded-2xl p-6">
            <h2 className="text-xl font-semibold mb-4">Быстрая установка</h2>
            <div className="space-y-4">
              <div>
                <h3 className="font-medium mb-2" style={{ color: '#00ff88' }}>Linux</h3>
                <code className="block p-3 rounded-lg text-sm font-mono"
                  style={{ background: 'rgba(255,255,255,0.05)', color: 'var(--muted-foreground)' }}>
                  curl -fsSL /api/install/linux | sudo bash
                </code>
              </div>
              <div>
                <h3 className="font-medium mb-2" style={{ color: '#00ff88' }}>Windows (PowerShell)</h3>
                <code className="block p-3 rounded-lg text-sm font-mono"
                  style={{ background: 'rgba(255,255,255,0.05)', color: 'var(--muted-foreground)' }}>
                  irm /api/install/windows | iex
                </code>
              </div>
            </div>
          </div>
        </motion.div>
      </div>
    </div>
  );
}
