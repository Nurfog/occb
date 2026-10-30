"use client";

/**
 * Módulos DLC (SaaS): el backend expone el catálogo con estado de licencia
 * en `GET /api/v1/modules`. Este contexto lo carga tras el login y ofrece:
 * - `useModules()`: lista + `licensed(slug)` + `enabled(slug)`.
 * - `<ModuleGate slug>` : renderiza children solo si el módulo está
 *   habilitado y licenciado; si no, muestra estado "DLC bloqueado" (402
 *   `module_locked` del backend) en vez de romper la navegación.
 */

import { createContext, useCallback, useContext, useEffect, useState } from "react";
import type { ReactNode } from "react";
import { useAuth } from "./auth";

export interface ModuleInfo {
  slug: string;
  name: string;
  description: string;
  version: string;
  enabled: boolean;
  route_prefix: string;
  licensed: boolean;
}

interface ModulesState {
  modules: ModuleInfo[];
  loading: boolean;
  /** Habilitado en catálogo Y cubierto por licencia (módulos base siempre). */
  open: (slug: string) => boolean;
  /** Solo cobertura de licencia (para mensajes de upsell). */
  licensed: (slug: string) => boolean;
  refresh: () => Promise<void>;
}

const ModulesContext = createContext<ModulesState>({
  modules: [],
  loading: true,
  open: () => false,
  licensed: () => false,
  refresh: async () => {},
});

export function ModulesProvider({ children }: { children: ReactNode }) {
  const { user, apiFetch } = useAuth();
  const [modules, setModules] = useState<ModuleInfo[]>([]);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    if (!user) {
      setModules([]);
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const list = await apiFetch<ModuleInfo[]>("/api/v1/modules");
      setModules(list);
    } catch {
      setModules([]);
    } finally {
      setLoading(false);
    }
  }, [user, apiFetch]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const open = useCallback(
    (slug: string) => {
      // Base incluida: crm/privacy no se gatean en el backend, siempre open.
      if (slug === "crm" || slug === "privacy") return true;
      const m = modules.find((x) => x.slug === slug);
      return !!m && m.enabled && m.licensed;
    },
    [modules]
  );

  const licensed = useCallback(
    (slug: string) => {
      if (slug === "crm" || slug === "privacy") return true;
      return !!modules.find((x) => x.slug === slug && x.licensed);
    },
    [modules]
  );

  return (
    <ModulesContext.Provider value={{ modules, loading, open, licensed, refresh }}>
      {children}
    </ModulesContext.Provider>
  );
}

export function useModules(): ModulesState {
  return useContext(ModulesContext);
}

/** Muestra `children` si el módulo está abierto; si no, tarjeta DLC bloqueado. */
export function ModuleGate({ slug, children }: { slug: string; children: ReactNode }) {
  const { open, loading, modules } = useModules();
  if (loading) return <p>Cargando módulos…</p>;
  if (open(slug)) return <>{children}</>;
  const meta = modules.find((x) => x.slug === slug);
  return (
    <section>
      <h2>Módulo bloqueado{meta ? `: ${meta.name}` : ""}</h2>
      <p>
        Este DLC requiere una licencia activa
        {meta ? "" : " (el módulo aún no está disponible)"}. Contacta al
        administrador para activarlo.
      </p>
    </section>
  );
}
