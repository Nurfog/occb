"use client";

/**
 * Página principal: login/registro si no hay sesión; perfil + logout si la hay.
 * El perfil se obtiene con GET /api/v1/auth/me (revalida el access en memoria).
 */

import { useEffect, useState } from "react";
import Link from "next/link";
import { useAuth } from "../lib/auth";
import { ModuleGate } from "../lib/modules";
import { ApiError } from "../lib/api";
import type { UserPublic } from "../lib/api";

export default function HomePage() {
  const { user, login, register, logout, loading, error, apiFetch } = useAuth();
  const [mode, setMode] = useState<"login" | "register">("login");
  const [email, setEmail] = useState("");
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [formError, setFormError] = useState<string | null>(null);

  // Perfil revalidado contra /auth/me + estado de la verificación.
  const [me, setMe] = useState<UserPublic | null>(null);
  const [meState, setMeState] = useState<"idle" | "loading" | "ok" | "error">("idle");

  useEffect(() => {
    if (!user) {
      setMe(null);
      setMeState("idle");
      return;
    }
    let alive = true;
    setMeState("loading");
    apiFetch<UserPublic>("/api/v1/auth/me")
      .then((u) => {
        if (alive) {
          setMe(u);
          setMeState("ok");
        }
      })
      .catch(() => {
        if (alive) setMeState("error");
      });
    return () => {
      alive = false;
    };
  }, [user, apiFetch]);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    setFormError(null);
    try {
      if (mode === "login") await login(email, password);
      else await register(email, name, password);
    } catch (err) {
      if (err instanceof ApiError) setFormError(`${err.code}: ${err.message}`);
      else setFormError("error de red");
    }
  }

  if (!user) {
    return (
      <main>
        <h1>OCCB</h1>
        <nav>
          <Link href="/users">Usuarios</Link> <Link href="/setup">Instalación</Link>
        </nav>
        <h2>{mode === "login" ? "Entrar" : "Crear cuenta"}</h2>
        <form onSubmit={onSubmit}>
          <input
            type="email"
            required
            placeholder="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            autoComplete="email"
          />
          {mode === "register" && (
            <input
              type="text"
              required
              placeholder="nombre"
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoComplete="name"
            />
          )}
          <input
            type="password"
            required
            minLength={mode === "register" ? 12 : 1}
            placeholder={mode === "register" ? "contraseña (12-128)" : "contraseña"}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete={mode === "login" ? "current-password" : "new-password"}
          />
          <button type="submit" disabled={loading}>
            {loading ? "…" : mode === "login" ? "Entrar" : "Registrarme"}
          </button>
        </form>
        <p>
          <button type="button" onClick={() => setMode(mode === "login" ? "register" : "login")}>
            {mode === "login" ? "¿Sin cuenta? Regístrate" : "¿Ya tienes cuenta? Entra"}
          </button>
        </p>
        {(formError ?? error) && <p className="error">{formError ?? error}</p>}
      </main>
    );
  }

  const profile = me ?? user;
  return (
    <main>
      <h1>OCCB</h1>
      <nav>
        <Link href="/users">Usuarios</Link>
      </nav>
      <h2>Perfil</h2>
      {meState === "loading" && <p className="muted">Verificando sesión…</p>}
      {meState === "error" && <p className="error">No se pudo revalidar /auth/me.</p>}
      <div className="card">
        <p>
          <strong>{profile.name}</strong> · {profile.email}
        </p>
        <p className="muted">
          {profile.id} · alta {profile.created_at}
        </p>
      </div>
      <button type="button" onClick={() => void logout()}>
        Salir
      </button>
      <h2>Módulos</h2>
      <ModuleGate slug="flow">
        <FlowPing />
      </ModuleGate>
    </main>
  );
}

function FlowPing() {
  const { apiFetch } = useAuth();
  const [text, setText] = useState("Verificando DLC flow…");
  useEffect(() => {
    apiFetch<{ module: string; status: string }>("/api/v1/flow/ping")
      .then((r) => setText(`Flow activo (${r.status})`))
      .catch((e: unknown) =>
        setText(e instanceof ApiError ? `Flow: ${e.message}` : "Flow: error")
      );
  }, [apiFetch]);
  return <p className="muted">{text}</p>;
}
