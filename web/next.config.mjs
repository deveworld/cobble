const basePath = process.env.NEXT_PUBLIC_BASE_PATH || "";

/** @type {import('next').NextConfig} */
const nextConfig = {
  output: "export",
  trailingSlash: true,
  allowedDevOrigins: ["127.0.0.1", "localhost"],
  basePath: basePath || undefined,
  assetPrefix: basePath ? `${basePath}/` : undefined,
  images: {
    unoptimized: true
  },
  webpack(config) {
    config.experiments = {
      ...config.experiments,
      asyncWebAssembly: true
    };
    config.output = {
      ...config.output,
      environment: {
        ...config.output?.environment,
        asyncFunction: true
      }
    };
    return config;
  }
};

export default nextConfig;
