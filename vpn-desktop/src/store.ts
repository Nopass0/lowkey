import { create } from 'zustand';
import { persist } from 'zustand/middleware';

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
}

export const useStore = create<AppState>()(
  persist(
    (set) => ({
      token: null,
      user: null,
      apiUrl: 'http://localhost:8080',
      connected: false,
      vpnIp: null,

      setToken: (token) => set({ token }),
      setUser: (user) => set({ user }),
      setConnected: (connected, vpnIp) => set({ connected, vpnIp: vpnIp || null }),
      setApiUrl: (apiUrl) => set({ apiUrl }),
      logout: () => set({ token: null, user: null, connected: false, vpnIp: null }),
    }),
    {
      name: 'lowkey-vpn-store',
    }
  )
);
