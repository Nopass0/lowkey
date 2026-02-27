import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { invoke } from '@tauri-apps/api/core';

interface User {
  id: number;
  login: string;
  balance: number;
  sub_status: string;
  sub_expires_at: string | null;
  sub_speed_mbps: number;
  referral_code: string | null;
  referral_balance: number;
  first_purchase_done: boolean;
}

// VITE_API_URL can be injected at build time via the VITE_API_URL env var
// (set automatically by build.sh / build.ps1 when a server IP is provided).
const BUILD_TIME_API_URL: string | undefined = import.meta.env.VITE_API_URL;

interface AppState {
  token: string | null;
  user: User | null;
  apiUrl: string;
  connected: boolean;
  vpnIp: string | null;

  setToken: (token: string) => void;
  setUser: (user: User) => void;
  setConnected: (connected: boolean, vpnIp?: string) => void;
  setApiUrl: (url: string) => void;
  logout: () => void;

  /** Apply the server IP baked in at build time, if not already configured. */
  applyBakedServerIp: () => Promise<void>;
}

export const useStore = create<AppState>()(
  persist(
    (set, get) => ({
      token: null,
      user: null,
      // Use build-time URL if provided, else fall back to localhost
      apiUrl: BUILD_TIME_API_URL || 'http://localhost:8080',
      connected: false,
      vpnIp: null,

      setToken: (token) => set({ token }),
      setUser: (user) => set({ user }),
      setConnected: (connected, vpnIp) => set({ connected, vpnIp: vpnIp || null }),
      setApiUrl: (apiUrl) => set({ apiUrl }),
      logout: () => set({ token: null, user: null, connected: false, vpnIp: null }),

      applyBakedServerIp: async () => {
        // Only override if still on the default localhost value
        const current = get().apiUrl;
        if (current !== 'http://localhost:8080') return;
        try {
          const bakedIp = await invoke<string | null>('get_baked_server_ip');
          if (bakedIp && bakedIp.length > 0) {
            set({ apiUrl: `http://${bakedIp}:8080` });
          }
        } catch {
          // Ignore — running in browser without Tauri runtime
        }
      },
    }),
    {
      name: 'lowkey-vpn-store',
    }
  )
);
