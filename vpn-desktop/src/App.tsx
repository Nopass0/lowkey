import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AnimatePresence, motion } from 'framer-motion';
import LoginView from './views/LoginView';
import MainView from './views/MainView';
import { useStore } from './store';

export default function App() {
  const { token, setUser, apiUrl } = useStore();

  useEffect(() => {
    if (token) {
      invoke<any>('get_user_info', { apiUrl, token })
        .then(user => setUser(user))
        .catch(() => useStore.getState().logout());
    }
  }, [token]);

  return (
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
  );
}
