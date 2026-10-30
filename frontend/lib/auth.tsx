"use client";

/**
 * Autenticación en memoria React.
 *
 * - Access token (15 min) y refresh token SOLO en `useState`/`useRef`.
 *   NUNCA en localStorage/sessionStorage (riesgo XSS).
 * - `apiFetch` añade `Bearer` y, ante un 401 con access en memoria,
 *   intenta UN refresh y reintenta la petición una sola vez.
 * - `logout` limpia toda la memoria de auth aunque el servidor falle.
 */

import { createContext, useCallback, useContext, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { ApiError, apiPost, apiPostRaw, requestJson } from "./api";
import type { UserPublic, TokenPair } from "./api";

interface AuthContextValue {
  user: UserPublic | null;
  /** Solo para depuración/mostrar estado; no persistir. */
  accessToken: string | null;
  refreshToken: string | null;
  loading: boolean;
  error: string | null;
  login: (email: string, password: string) => Promise<void>;
  register: (email: string, name: string, password: string) => Promise<void>;
  logout: () => Promise<void>;
  /** fetch autenticado con refresh-y-reintento (1 vez) ante 401. */
  apiFetch: <T>(path: string, init?: { method?: string; body?: unknown }) => Promise<T>;
}

const AuthContext = createContext<AuthContextValue | null>(null);

function applyPair(
  pair: TokenPair,
  setUser: (u: UserPublic) => void,
  setAccess: (t: string) => void,
  setRefresh: (t: string) => void,
  accessRef: { current: string | null },
  refreshRef: { current: string | null },
): void {
  setUser(pair.user);
  setAccess(pair.access_token);
  setRefresh(pair.refresh_token);
  accessRef.current = pair.access_token;
  refreshRef.current = pair.refresh_token;
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<UserPublic | null>(null);
  const [accessToken, setAccessToken] = useState<string | null>(null);
  const [refreshToken, setRefreshToken] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Espejos en refs para usar dentro de `apiFetch` sin closures obsoletos.
  const accessRef = useRef<string | null>(null);
  const refreshRef = useRef<string | null>(null);

  const clearAuth = useCallback(() => {
    setUser(null);
    setAccessToken(null);
    setRefreshToken(null);
    accessRef.current = null;
    refreshRef.current = null;
  }, []);

  /** Rota el par de tokens. Lanza si el refresh falla (y limpia auth). */
  const doRefresh = useCallback(async (): Promise<string> => {
    const rt = refreshRef.current;
    if (!rt) throw new ApiError(401, "unauthorized", "sin sesión");
    let pair: TokenPair;
    try {
      pair = await apiPost<TokenPair>("/api/v1/auth/refresh", { refresh_token: rt });
    } catch {
      clearAuth();
      throw new ApiError(401, "unauthorized", "sesión expirada");
    }
    setUser(pair.user);
    setAccessToken(pair.access_token);
    setRefreshToken(pair.refresh_token);
    accessRef.current = pair.access_token;
    refreshRef.current = pair.refresh_token;
    return pair.access_token;
  }, [clearAuth]);

  const apiFetch = useCallback(
    async <T,>(path: string, init: { method?: string; body?: unknown } = {}): Promise<T> => {
      try {
        return await requestJson<T>(path, {
          method: init.method ?? "GET",
          body: init.body,
          token: accessRef.current,
        });
      } catch (e) {
        // Un solo reintento tras refresh, solo si había access token en memoria.
        if (e instanceof ApiError && e.status === 401 && accessRef.current) {
          const fresh = await doRefresh();
          return await requestJson<T>(path, {
            method: init.method ?? "GET",
            body: init.body,
            token: fresh,
          });
        }
        throw e;
      }
    },
    [doRefresh],
  );

  const login = useCallback(
    async (email: string, password: string) => {
      setLoading(true);
      setError(null);
      try {
        const pair = await apiPost<TokenPair>("/api/v1/auth/login", { email, password });
        applyPair(pair, setUser, setAccessToken, setRefreshToken, accessRef, refreshRef);
      } catch (e) {
        setError(e instanceof ApiError ? e.message : "error de red");
        throw e;
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  const register = useCallback(
    async (email: string, name: string, password: string) => {
      setLoading(true);
      setError(null);
      try {
        const pair = await apiPost<TokenPair>("/api/v1/auth/register", { email, name, password });
        applyPair(pair, setUser, setAccessToken, setRefreshToken, accessRef, refreshRef);
      } catch (e) {
        setError(e instanceof ApiError ? e.message : "error de red");
        throw e;
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  const logout = useCallback(async () => {
    const at = accessRef.current;
    const rt = refreshRef.current;
    try {
      if (at && rt) {
        await apiPostRaw("/api/v1/auth/logout", { refresh_token: rt }, { token: at });
      }
    } catch {
      // El logout local se completa igual: la memoria se limpia siempre.
    } finally {
      clearAuth();
    }
  }, [clearAuth]);

  const value = useMemo<AuthContextValue>(
    () => ({
      user,
      accessToken,
      refreshToken,
      loading,
      error,
      login,
      register,
      logout,
      apiFetch,
    }),
    [user, accessToken, refreshToken, loading, error, login, register, logout, apiFetch],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth debe usarse dentro de <AuthProvider>");
  return ctx;
}
