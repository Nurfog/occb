"use client";

/**
 * Instalador inicial estilo WordPress (`GET/POST /api/v1/setup/*`, público).
 * Paso 1: checklist de requisitos. Paso 2: nombre del sitio + primer
 * administrador (root). Tras instalar, el endpoint se bloquea (403) y esta
 * página muestra "ya instalado".
 */

import { useEffect, useState } from "react";
import Link from "next/link";
import { getApiBaseUrl } from "../../lib/api";

interface SetupCheck {
  ok: boolean;
  detail: string;
}

interface SetupStatus {
  installed: boolean;
  version: string;
  checks: [string, SetupCheck][];
}

type Phase =
  | { step: "loading" }
  | { step: "ready" }
  | { step: "installing" }
  | { step: "done" }
  | { step: "failed"; message: string };

export default function SetupPage() {
  const [status, setStatus] = useState<SetupStatus | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [phase, setPhase] = useState<Phase>({ step: "loading" });
  const [siteName, setSiteName] = useState("");
  const [adminName, setAdminName] = useState("");
  const [adminEmail, setAdminEmail] = useState("");
  const [adminPassword, setAdminPassword] = useState("");
  const [adminPassword2, setAdminPassword2] = useState("");
  const [formError, setFormError] = useState<string | null>(null);

  useEffect(() => {
    fetch(`${getApiBaseUrl()}/api/v1/setup/status`)
      .then(async (res) => {
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        setStatus((await res.json()) as SetupStatus);
        setPhase({ step: "ready" });
      })
      .catch(() => {
        setStatusError("No se pudo contactar al backend.");
        setPhase({ step: "failed", message: "No se pudo contactar al backend." });
      });
  }, []);

  async function onInstall(e: React.FormEvent) {
    e.preventDefault();
    setFormError(null);
    if (adminPassword !== adminPassword2) {
      setFormError("Las contraseñas no coinciden.");
      return;
    }
    setPhase({ step: "installing" });
    try {
      const res = await fetch(`${getApiBaseUrl()}/api/v1/setup/install`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          site_name: siteName,
          admin_name: adminName,
          admin_email: adminEmail,
          admin_password: adminPassword,
        }),
      });
      if (res.status === 403) {
        setPhase({ step: "failed", message: "El sistema ya está instalado." });
        return;
      }
      if (!res.ok) {
        const data = (await res.json().catch(() => null)) as {
          error?: { message?: string };
        } | null;
        throw new Error(data?.error?.message ?? `HTTP ${res.status}`);
      }
      const st = status;
      if (st) setStatus({ ...st, installed: true });
      setPhase({ step: "done" });
    } catch (err) {
      setPhase({
        step: "failed",
        message: err instanceof Error ? err.message : "Error al instalar.",
      });
    }
  }

  return (
    <main>
      <h1>Instalación de OCCB</h1>

      {phase.step === "loading" && <p className="muted">Verificando el sistema…</p>}

      {(phase.step === "failed" || statusError) && phase.step !== "done" && (
        <p className="error">{phase.step === "failed" ? phase.message : statusError}</p>
      )}

      {status && (
        <section>
          <h2>Requisitos</h2>
          <ul>
            {status.checks.map(([name, check]) => (
              <li key={name}>
                {check.ok ? "✓" : "✗"} <strong>{name}</strong> · {check.detail}
              </li>
            ))}
          </ul>
          <p className="muted">Versión {status.version}</p>
        </section>
      )}

      {status?.installed && phase.step !== "done" && (
        <section>
          <h2>Ya instalado</h2>
          <p>
            El sistema ya tiene un administrador. <Link href="/">Ir al inicio</Link>
          </p>
        </section>
      )}

      {status && !status.installed && (phase.step === "ready" || phase.step === "installing") && (
        <section>
          <h2>Crear el administrador inicial</h2>
          <form onSubmit={onInstall}>
            <label>
              Nombre del sitio
              <input
                value={siteName}
                onChange={(e) => setSiteName(e.target.value)}
                placeholder="Mi Colegio"
                required
                minLength={1}
                maxLength={100}
              />
            </label>
            <label>
              Nombre del administrador
              <input
                value={adminName}
                onChange={(e) => setAdminName(e.target.value)}
                required
                minLength={1}
                maxLength={100}
              />
            </label>
            <label>
              Email del administrador
              <input
                type="email"
                value={adminEmail}
                onChange={(e) => setAdminEmail(e.target.value)}
                required
              />
            </label>
            <label>
              Contraseña (12+ caracteres)
              <input
                type="password"
                value={adminPassword}
                onChange={(e) => setAdminPassword(e.target.value)}
                required
                minLength={12}
                maxLength={128}
              />
            </label>
            <label>
              Repetir contraseña
              <input
                type="password"
                value={adminPassword2}
                onChange={(e) => setAdminPassword2(e.target.value)}
                required
              />
            </label>
            {formError && <p className="error">{formError}</p>}
            <button type="submit" disabled={phase.step === "installing"}>
              {phase.step === "installing" ? "Instalando…" : "Instalar"}
            </button>
          </form>
        </section>
      )}

      {phase.step === "done" && (
        <section>
          <h2>Instalación completa</h2>
          <p>
            El administrador fue creado. <Link href="/">Inicia sesión</Link> para
            entrar.
          </p>
        </section>
      )}
    </main>
  );
}
