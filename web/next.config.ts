import type { NextConfig } from "next";
import path from "path";

const nextConfig: NextConfig = {
  output: "standalone",
  allowedDevOrigins: ["localhost", "127.0.0.1", "89.169.54.87"],
  turbopack: {
    root: __dirname,
  },
  env: {
    NEXT_PUBLIC_API_URL:
      process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080",
  },
  webpack(config) {
    // Restrict module resolution to this package's own node_modules.
    // Without this, webpack climbs to parent directories and fails to find
    // tailwindcss when the project is nested inside a larger monorepo folder.
    config.resolve.modules = [
      path.resolve(__dirname, "node_modules"),
      "node_modules",
    ];
    return config;
  },
};

export default nextConfig;
