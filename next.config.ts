import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "standalone",
  // Evita que Next infiera otra raíz por lockfiles ajenos fuera del proyecto.
  outputFileTracingRoot: __dirname,
  // El build debe pasar con CERO errores de TypeScript; el lint no bloquea.
  eslint: { ignoreDuringBuilds: true },
};

export default nextConfig;
