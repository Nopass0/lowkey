import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatDate(dateStr: string | null | undefined): string {
  if (!dateStr) return "—";
  return new Date(dateStr).toLocaleDateString("ru-RU", {
    day: "2-digit",
    month: "long",
    year: "numeric",
  });
}

export function formatDateTime(dateStr: string | null | undefined): string {
  if (!dateStr) return "—";
  return new Date(dateStr).toLocaleString("ru-RU", {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function formatRub(amount: number): string {
  return new Intl.NumberFormat("ru-RU", {
    style: "currency",
    currency: "RUB",
    minimumFractionDigits: 2,
  }).format(amount);
}

export function formatSpeed(mbps: number): string {
  if (mbps === 0) return "Без ограничений";
  return `${mbps} Мбит/с`;
}

export function getDaysLeft(expiresAt: string | null | undefined): number {
  if (!expiresAt) return 0;
  const diff = new Date(expiresAt).getTime() - Date.now();
  return Math.max(0, Math.ceil(diff / (1000 * 60 * 60 * 24)));
}

export function isExpiringSoon(expiresAt: string | null | undefined, days = 5): boolean {
  const left = getDaysLeft(expiresAt);
  return left > 0 && left <= days;
}

export function getStatusLabel(status: string): string {
  switch (status) {
    case 'active': return 'Активна';
    case 'expired': return 'Истекла';
    case 'inactive': return 'Неактивна';
    case 'paid': return 'Оплачено';
    case 'pending': return 'Ожидание';
    case 'failed': return 'Ошибка';
    case 'completed': return 'Выплачено';
    case 'rejected': return 'Отклонено';
    case 'processing': return 'Обработка';
    default: return status;
  }
}
