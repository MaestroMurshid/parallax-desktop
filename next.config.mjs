/** @type {import('next').NextConfig} */
// §9.1 — Next is used as a static SPA generator, not a framework.
// Lost by design: API routes, server components doing server work, middleware,
// SSR, ISR. Tauri commands are the API layer (see lib/bridge).
const nextConfig = {
  output: 'export',
  images: { unoptimized: true },
  trailingSlash: true,
  devIndicators: false,
};

export default nextConfig;
