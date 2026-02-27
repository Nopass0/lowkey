import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Lowkey VPN — Безопасный и быстрый VPN",
  description: "Надёжный VPN-сервис с доступными тарифами и оплатой через СБП",
  keywords: "VPN, Lowkey, безопасность, приватность, СБП, оплата",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="ru">
      <body className="antialiased">
        {children}
      </body>
    </html>
  );
}
