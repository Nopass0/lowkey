"use client";

import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import Link from "next/link";
import {
  Shield,
  Download,
  Monitor,
  Server,
  Smartphone,
  CheckCircle,
  Clock,
  FileText,
  Hash,
} from "lucide-react";
import { versions, AppRelease } from "@/lib/api";

const PLATFORMS = [
  {
    key: "windows",
    label: "Windows",
    sub: "Windows 10/11 (x64)",
    icon: Monitor,
    ext: ".exe",
  },
  {
    key: "linux",
    label: "Linux",
    sub: "Ubuntu / Debian / Fedora (x64)",
    icon: Server,
    ext: "",
  },
  {
    key: "android",
    label: "Android",
    sub: "Android 8.0+",
    icon: Smartphone,
    ext: ".apk",
  },
];

function formatBytes(bytes: number | null | undefined): string {
  if (!bytes) return "";
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export default function DownloadsPage() {
  const [releases, setReleases] = useState<AppRelease[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    versions
      .all()
      .then((r) => setReleases(r.releases))
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  return (
    <div
      className="min-h-screen px-4 py-12"
      style={{ background: "var(--background)" }}
    >
      {/* Background */}
      <div className="fixed inset-0 overflow-hidden pointer-events-none">
        <div
          className="absolute top-1/4 left-1/4 w-96 h-96 rounded-full opacity-5 blur-3xl"
          style={{ background: "radial-gradient(circle,#3b82f6,transparent)" }}
        />
        <div
          className="absolute bottom-1/4 right-1/4 w-96 h-96 rounded-full opacity-5 blur-3xl"
          style={{ background: "radial-gradient(circle,#0066ff,transparent)" }}
        />
      </div>

      <div className="max-w-4xl mx-auto relative">
        {/* Logo */}
        <div className="flex items-center gap-2 mb-12">
          <Link href="/" className="flex items-center gap-2">
            <div
              className="w-8 h-8 rounded-lg flex items-center justify-center"
              style={{ background: "linear-gradient(135deg,#3b82f6,#2563eb)" }}
            >
              <Shield className="w-4 h-4 text-white" />
            </div>
            <span className="font-bold gradient-text">Lowkey VPN</span>
          </Link>
        </div>

        <motion.div
          initial={{ opacity: 0, y: 30 }}
          animate={{ opacity: 1, y: 0 }}
        >
          <h1 className="text-5xl font-black mb-3">Скачать</h1>
          <p
            className="text-lg mb-10"
            style={{ color: "var(--muted-foreground)" }}
          >
            Выберите версию для вашей платформы. Приложения автоматически
            обновляются до последней версии.
          </p>

          {/* Platform cards */}
          <div className="space-y-4 mb-12">
            {PLATFORMS.map((platform, i) => {
              const release = releases.find((r) => r.platform === platform.key);
              const PIcon = platform.icon;

              return (
                <motion.div
                  key={platform.key}
                  initial={{ opacity: 0, x: -20 }}
                  animate={{ opacity: 1, x: 0 }}
                  transition={{ delay: i * 0.08 }}
                  className="glass rounded-2xl overflow-hidden"
                >
                  <div className="p-6 flex items-center justify-between flex-wrap gap-4">
                    <div className="flex items-center gap-4">
                      <div
                        className="w-14 h-14 rounded-xl flex items-center justify-center"
                        style={{ background: "rgba(59,130,246,0.1)" }}
                      >
                        <PIcon
                          className="w-7 h-7"
                          style={{ color: "#60a5fa" }}
                        />
                      </div>
                      <div>
                        <h3 className="font-bold text-xl">{platform.label}</h3>
                        <p
                          className="text-sm"
                          style={{ color: "var(--muted-foreground)" }}
                        >
                          {platform.sub}
                        </p>
                        {release ? (
                          <div
                            className="flex items-center gap-3 mt-1 text-xs"
                            style={{ color: "var(--muted-foreground)" }}
                          >
                            <span className="flex items-center gap-1">
                              <CheckCircle
                                className="w-3 h-3"
                                style={{ color: "#60a5fa" }}
                              />
                              v{release.version}
                            </span>
                            {release.file_size_bytes && (
                              <span>
                                {formatBytes(release.file_size_bytes)}
                              </span>
                            )}
                            <span className="flex items-center gap-1">
                              <Clock className="w-3 h-3" />
                              {new Date(release.released_at).toLocaleDateString(
                                "ru-RU",
                              )}
                            </span>
                          </div>
                        ) : loading ? (
                          <div
                            className="text-xs mt-1"
                            style={{ color: "var(--muted-foreground)" }}
                          >
                            Загрузка...
                          </div>
                        ) : (
                          <div
                            className="text-xs mt-1"
                            style={{ color: "var(--muted-foreground)" }}
                          >
                            Скоро
                          </div>
                        )}
                      </div>
                    </div>

                    {release ? (
                      <a
                        href={release.download_url}
                        className="btn btn-primary flex items-center gap-2 glow-blue"
                        download={release.file_name || undefined}
                      >
                        <Download className="w-4 h-4" />
                        Скачать
                        {release.file_name
                          ? ` ${release.file_name.split(".").pop()?.toUpperCase()}`
                          : ""}
                      </a>
                    ) : (
                      <button
                        disabled
                        className="btn btn-secondary opacity-50"
                        style={{ cursor: "not-allowed" }}
                      >
                        Скоро
                      </button>
                    )}
                  </div>

                  {/* Changelog accordion */}
                  {release?.changelog && (
                    <div
                      className="px-6 pb-5 border-t"
                      style={{ borderColor: "var(--border)" }}
                    >
                      <details className="mt-4 group">
                        <summary
                          className="flex items-center gap-2 text-sm cursor-pointer select-none"
                          style={{ color: "var(--muted-foreground)" }}
                        >
                          <FileText className="w-4 h-4" />
                          Что нового в v{release.version}
                        </summary>
                        <div
                          className="mt-3 text-sm whitespace-pre-wrap leading-relaxed"
                          style={{ color: "var(--muted-foreground)" }}
                        >
                          {release.changelog}
                        </div>
                      </details>
                      {release.sha256_checksum && (
                        <div
                          className="mt-3 flex items-start gap-2 text-xs font-mono p-2 rounded-lg"
                          style={{
                            background: "rgba(255,255,255,0.03)",
                            color: "var(--muted-foreground)",
                          }}
                        >
                          <Hash className="w-3.5 h-3.5 mt-0.5 flex-shrink-0" />
                          <span className="break-all">
                            SHA256: {release.sha256_checksum}
                          </span>
                        </div>
                      )}
                    </div>
                  )}
                </motion.div>
              );
            })}
          </div>

          {/* Quick install */}
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.4 }}
            className="glass rounded-2xl p-6 mb-8"
          >
            <h2 className="text-xl font-bold mb-5">
              Быстрая установка через терминал
            </h2>
            <div className="space-y-4">
              <div>
                <div
                  className="flex items-center gap-2 text-sm font-medium mb-2"
                  style={{ color: "#60a5fa" }}
                >
                  <Server className="w-4 h-4" /> Linux / macOS
                </div>
                <code
                  className="block p-3 rounded-xl text-sm font-mono overflow-x-auto"
                  style={{
                    background: "rgba(0,0,0,0.5)",
                    color: "var(--muted-foreground)",
                  }}
                >
                  curl -fsSL https://get.lowkeyvpn.com/linux | sudo bash
                </code>
              </div>
              <div>
                <div
                  className="flex items-center gap-2 text-sm font-medium mb-2"
                  style={{ color: "#60a5fa" }}
                >
                  <Monitor className="w-4 h-4" /> Windows PowerShell (от
                  Администратора)
                </div>
                <code
                  className="block p-3 rounded-xl text-sm font-mono overflow-x-auto"
                  style={{
                    background: "rgba(0,0,0,0.5)",
                    color: "var(--muted-foreground)",
                  }}
                >
                  irm https://get.lowkeyvpn.com/windows | iex
                </code>
              </div>
            </div>
          </motion.div>

          {/* Auto-update info */}
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.5 }}
            className="rounded-2xl p-5 flex items-start gap-3"
            style={{
              background: "rgba(59,130,246,0.06)",
              border: "1px solid rgba(59,130,246,0.2)",
            }}
          >
            <CheckCircle
              className="w-5 h-5 flex-shrink-0 mt-0.5"
              style={{ color: "#60a5fa" }}
            />
            <div>
              <div className="font-semibold" style={{ color: "#60a5fa" }}>
                Автообновление
              </div>
              <div
                className="text-sm mt-1"
                style={{ color: "var(--muted-foreground)" }}
              >
                Приложения проверяют наличие обновлений при каждом запуске и
                предлагают обновиться до актуальной версии. Обновление
                загружается в фоне — VPN продолжает работать.
              </div>
            </div>
          </motion.div>
        </motion.div>
      </div>
    </div>
  );
}
