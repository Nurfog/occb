import type { Metadata } from "next";
import { AuthProvider } from "../lib/auth";
import { ModulesProvider } from "../lib/modules";
import "./globals.css";

export const metadata: Metadata = {
  title: "OCCB",
  description: "Frontend OCCB",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="es">
      <body>
        <AuthProvider>
          <ModulesProvider>{children}</ModulesProvider>
        </AuthProvider>
      </body>
    </html>
  );
}
