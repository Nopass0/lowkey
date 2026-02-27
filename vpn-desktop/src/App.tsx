import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-shell';
import { AnimatePresence, motion } from 'framer-motion';
import LoginView from './views/LoginView';
import MainView from './views/MainView';
import { useStore } from './store';

const APP_VERSION = '1.0.0';

interface ReleaseInfo {
  version: string;
  download_url: string;
  changelog: string | null;
}

export default function App() {
  const { token, setUser, apiUrl } = useStore();
  const [updateInfo, setUpdateInfo] = useState<ReleaseInfo | null>(null);

  useEffect(() => {
    if (token) {
      invoke<any>('get_user_info', { apiUrl, token })
        .then(user => setUser(user))
        .catch(() => useStore.getState().logout());
    }
  }, [token]);

  // Check for updates once on mount
  useEffect(() => {
    invoke<ReleaseInfo | null>('check_for_update', {
      apiUrl,
      currentVersion: APP_VERSION,
    }).then(info => {
      if (info) setUpdateInfo(info);
    }).catch(() => {});
  }, []);

  return (
    <>
      <AnimatePresence mode="wait">
        {token ? (
          <motion.div key="main" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}>
            <MainView />
          </motion.div>
        ) : (
          <motion.div key="login" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}>
            <LoginView />
          </motion.div>
        )}
      </AnimatePresence>

      {/* Update available dialog */}
      {updateInfo && (
        <div style={{
          position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.7)',
          display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 9999,
        }}>
          <div style={{
            background: '#0d0d1f', border: '1px solid rgba(0,255,136,0.3)',
            borderRadius: 16, padding: 28, maxWidth: 360, width: '90%',
          }}>
            <div style={{ fontWeight: 700, fontSize: 18, marginBottom: 8, color: '#f0f4ff' }}>
              Доступно обновление v{updateInfo.version}
            </div>
            <div style={{ color: '#8892b0', fontSize: 14, marginBottom: 16, lineHeight: 1.6 }}>
              Установите последнюю версию для улучшений и исправлений безопасности.
              {updateInfo.changelog && (
                <div style={{ marginTop: 8, whiteSpace: 'pre-wrap', fontSize: 12 }}>
                  {updateInfo.changelog}
                </div>
              )}
            </div>
            <div style={{ display: 'flex', gap: 10 }}>
              <button
                onClick={() => open(updateInfo.download_url).finally(() => setUpdateInfo(null))}
                style={{
                  flex: 1, padding: '10px 0', borderRadius: 10, border: 'none',
                  background: 'linear-gradient(135deg,#00ff88,#0066ff)',
                  color: '#000', fontWeight: 700, cursor: 'pointer', fontSize: 14,
                }}
              >
                Скачать
              </button>
              <button
                onClick={() => setUpdateInfo(null)}
                style={{
                  flex: 1, padding: '10px 0', borderRadius: 10, border: '1px solid rgba(255,255,255,0.1)',
                  background: 'transparent', color: '#8892b0', cursor: 'pointer', fontSize: 14,
                }}
              >
                Позже
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
