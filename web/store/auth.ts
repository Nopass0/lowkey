import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { UserPublic, auth } from '@/lib/api';

interface AuthState {
  token: string | null;
  user: UserPublic | null;
  adminToken: string | null;
  isLoading: boolean;

  login: (login: string, password: string) => Promise<void>;
  register: (login: string, password: string, referralCode?: string) => Promise<void>;
  logout: () => void;
  setAdminToken: (token: string) => void;
  refreshUser: () => Promise<void>;
}

export const useAuthStore = create<AuthState>()(
  persist(
    (set, get) => ({
      token: null,
      user: null,
      adminToken: null,
      isLoading: false,

      login: async (login, password) => {
        set({ isLoading: true });
        try {
          const res = await auth.login(login, password);
          set({ token: res.token, user: res.user });
        } finally {
          set({ isLoading: false });
        }
      },

      register: async (login, password, referralCode) => {
        set({ isLoading: true });
        try {
          const res = await auth.register(login, password, referralCode);
          set({ token: res.token, user: res.user });
        } finally {
          set({ isLoading: false });
        }
      },

      logout: () => set({ token: null, user: null, adminToken: null }),

      setAdminToken: (token) => set({ adminToken: token }),

      refreshUser: async () => {
        const { token } = get();
        if (!token) return;
        try {
          const user = await auth.me(token);
          set({ user });
        } catch {
          // Token might be invalid
        }
      },
    }),
    {
      name: 'lowkey-auth',
      partialize: (state) => ({
        token: state.token,
        user: state.user,
        adminToken: state.adminToken,
      }),
    }
  )
);
