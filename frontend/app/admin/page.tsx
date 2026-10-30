"use client";

/**
 * Dashboard del superadmin: empresas + sus usuarios/admins.
 * Requiere sesión y flag `is_superadmin` (GET /api/v1/auth/me).
 */

import { useCallback, useEffect, useState } from "react";
import Link from "next/link";
import { useAuth } from "../../lib/auth";
import { ApiError } from "../../lib/api";
import type { UserPublic } from "../../lib/api";

interface AdminTenant {
  id: string;
  name: string;
  slug: string;
  status: string;
  contact_name: string | null;
  contact_email: string | null;
  users: number;
  created_at: string;
}

interface TenantUser {
  id: string;
  email: string;
  name: string;
  created_at: string;
}

function fmtError(e: unknown): string {
  if (e instanceof ApiError) return `${e.code}: ${e.message}`;
  return "error de red";
}

export default function AdminPage() {
  const { user, apiFetch } = useAuth();

  const [me, setMe] = useState<UserPublic | null>(null);
  const [meState, setMeState] = useState<"idle" | "loading" | "error">("idle");
  const [meError, setMeError] = useState<string | null>(null);

  const [tenants, setTenants] = useState<AdminTenant[]>([]);
  const [tenantsLoading, setTenantsLoading] = useState(false);
  const [tenantsError, setTenantsError] = useState<string | null>(null);
  const [opError, setOpError] = useState<string | null>(null);

  // Crear empresa.
  const [tName, setTName] = useState("");
  const [tSlug, setTSlug] = useState("");
  const [tContactName, setTContactName] = useState("");
  const [tContactEmail, setTContactEmail] = useState("");
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

  // Empresa seleccionada + usuarios.
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [tenantUsers, setTenantUsers] = useState<TenantUser[]>([]);
  const [usersLoading, setUsersLoading] = useState(false);
  const [usersError, setUsersError] = useState<string | null>(null);

  // Crear admin de la empresa seleccionada.
  const [aEmail, setAEmail] = useState("");
  const [aName, setAName] = useState("");
  const [aPassword, setAPassword] = useState("");
  const [aConfirm, setAConfirm] = useState("");
  const [creatingAdmin, setCreatingAdmin] = useState(false);
  const [adminError, setAdminError] = useState<string | null>(null);

  useEffect(() => {
    if (!user) {
      setMe(null);
      setMeState("idle");
      return;
    }
    let alive = true;
    setMeState("loading");
    setMeError(null);
    apiFetch<UserPublic>("/api/v1/auth/me")
      .then((u) => {
        if (alive) {
          setMe(u);
          setMeState("idle");
        }
      })
      .catch((e) => {
        if (alive) {
          setMeError(fmtError(e));
          setMeState("error");
        }
      });
    return () => {
      alive = false;
    };
  }, [user, apiFetch]);

  const loadTenants = useCallback(async () => {
    setTenantsLoading(true);
    setTenantsError(null);
    try {
      const res = await apiFetch<AdminTenant[]>("/api/v1/admin/tenants");
      setTenants(res);
    } catch (e) {
      setTenantsError(fmtError(e));
    } finally {
      setTenantsLoading(false);
    }
  }, [apiFetch]);

  const loadTenantUsers = useCallback(
    async (tenantId: string) => {
      setUsersLoading(true);
      setUsersError(null);
      try {
        const res = await apiFetch<TenantUser[]>(
          `/api/v1/admin/tenants/${encodeURIComponent(tenantId)}/users`
        );
        setTenantUsers(res);
      } catch (e) {
        setTenantUsers([]);
        setUsersError(fmtError(e));
      } finally {
        setUsersLoading(false);
      }
    },
    [apiFetch]
  );

  const isSuper = me?.is_superadmin === true;

  useEffect(() => {
    if (me && isSuper) void loadTenants();
  }, [me, isSuper, loadTenants]);

  useEffect(() => {
    if (selectedId && isSuper) void loadTenantUsers(selectedId);
    else {
      setTenantUsers([]);
      setUsersError(null);
    }
  }, [selectedId, isSuper, loadTenantUsers]);

  async function onCreateTenant(e: React.FormEvent) {
    e.preventDefault();
    setCreateError(null);
    const name = tName.trim();
    if (!name) {
      setCreateError("validation: el nombre es obligatorio");
      return;
    }
    setCreating(true);
    try {
      const body: Record<string, string> = { name };
      if (tSlug.trim()) body["slug"] = tSlug.trim();
      if (tContactName.trim()) body["contact_name"] = tContactName.trim();
      if (tContactEmail.trim()) body["contact_email"] = tContactEmail.trim();
      await apiFetch<AdminTenant>("/api/v1/admin/tenants", {
        method: "POST",
        body,
      });
      setTName("");
      setTSlug("");
      setTContactName("");
      setTContactEmail("");
      await loadTenants();
    } catch (err) {
      setCreateError(fmtError(err));
    } finally {
      setCreating(false);
    }
  }

  async function onSetStatus(id: string, status: string) {
    setOpError(null);
    try {
      await apiFetch<void>(`/api/v1/admin/tenants/${encodeURIComponent(id)}`, {
        method: "PATCH",
        body: { status },
      });
      await loadTenants();
    } catch (err) {
      setOpError(fmtError(err));
    }
  }

  async function onDelete(id: string) {
    setOpError(null);
    try {
      await apiFetch<void>(`/api/v1/admin/tenants/${encodeURIComponent(id)}`, {
        method: "DELETE",
      });
      if (selectedId === id) setSelectedId(null);
      await loadTenants();
    } catch (err) {
      setOpError(fmtError(err));
    }
  }

  async function onCreateAdmin(e: React.FormEvent) {
    e.preventDefault();
    setAdminError(null);
    if (!selectedId) return;
    if (aPassword.length < 12) {
      setAdminError("validation: la clave debe tener 12+ caracteres");
      return;
    }
    if (aPassword !== aConfirm) {
      setAdminError("validation: la confirmación no coincide");
      return;
    }
    setCreatingAdmin(true);
    try {
      await apiFetch<TenantUser>(
        `/api/v1/admin/tenants/${encodeURIComponent(selectedId)}/admins`,
        {
          method: "POST",
          body: { email: aEmail.trim(), name: aName.trim(), password: aPassword },
        }
      );
      setAEmail("");
      setAName("");
      setAPassword("");
      setAConfirm("");
      await loadTenantUsers(selectedId);
      await loadTenants();
    } catch (err) {
      setAdminError(fmtError(err));
    } finally {
      setCreatingAdmin(false);
    }
  }

  if (!user) {
    return (
      <main>
        <h1>Administración</h1>
        <p>
          Necesitas sesión. <Link href="/">Ir al login</Link>
        </p>
      </main>
    );
  }

  if (meState === "loading") {
    return (
      <main>
        <h1>Administración</h1>
        <p className="muted">Verificando sesión…</p>
      </main>
    );
  }

  if (meState === "error") {
    return (
      <main>
        <h1>Administración</h1>
        <p className="error">{meError ?? "No se pudo revalidar /auth/me."}</p>
      </main>
    );
  }

  if (!me || !isSuper) {
    return (
      <main>
        <h1>Administración</h1>
        <p>Acceso restringido al superadmin</p>
      </main>
    );
  }

  const selected = tenants.find((t) => t.id === selectedId) ?? null;

  return (
    <main>
      <h1>Administración</h1>
      <nav>
        <Link href="/">Perfil</Link>
      </nav>

      <h2>Empresas</h2>
      {tenantsLoading && <p className="muted">Cargando…</p>}
      {tenantsError && <p className="error">{tenantsError}</p>}
      {opError && <p className="error">{opError}</p>}
      {!tenantsLoading && !tenantsError && tenants.length === 0 && (
        <p className="muted">Sin empresas.</p>
      )}
      {tenants.length > 0 && (
        <table>
          <thead>
            <tr>
              <th>Nombre</th>
              <th>Slug</th>
              <th>Estado</th>
              <th>Usuarios</th>
              <th>Acciones</th>
            </tr>
          </thead>
          <tbody>
            {tenants.map((t) => (
              <tr key={t.id}>
                <td>
                  <button type="button" onClick={() => setSelectedId(t.id)}>
                    {t.name}
                  </button>
                </td>
                <td>{t.slug}</td>
                <td>{t.status}</td>
                <td>{t.users}</td>
                <td>
                  {(t.status === "pendiente" || t.status === "suspendida") && (
                    <button type="button" onClick={() => void onSetStatus(t.id, "activa")}>
                      Habilitar
                    </button>
                  )}{" "}
                  {(t.status === "pendiente" || t.status === "activa") && (
                    <button
                      type="button"
                      onClick={() => void onSetStatus(t.id, "suspendida")}
                    >
                      Suspender
                    </button>
                  )}{" "}
                  <button type="button" onClick={() => void onDelete(t.id)}>
                    Borrar
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <h2>Crear empresa</h2>
      <form onSubmit={onCreateTenant}>
        <input
          placeholder="nombre (obligatorio)"
          value={tName}
          onChange={(e) => setTName(e.target.value)}
          required
        />
        <input
          placeholder="slug (opcional)"
          value={tSlug}
          onChange={(e) => setTSlug(e.target.value)}
        />
        <input
          placeholder="contacto nombre (opcional)"
          value={tContactName}
          onChange={(e) => setTContactName(e.target.value)}
        />
        <input
          type="email"
          placeholder="contacto email (opcional)"
          value={tContactEmail}
          onChange={(e) => setTContactEmail(e.target.value)}
        />
        <button type="submit" disabled={creating}>
          {creating ? "…" : "Crear"}
        </button>
      </form>
      {createError && <p className="error">{createError}</p>}

      <h2>Usuarios de la empresa</h2>
      {!selected && <p className="muted">Selecciona una empresa de la tabla.</p>}
      {selected && (
        <>
          <div className="card">
            <p>
              <strong>{selected.name}</strong> · {selected.slug} · {selected.status}
            </p>
            <p className="muted">
              {selected.id} · {selected.users} usuarios
            </p>
          </div>
          {usersLoading && <p className="muted">Cargando usuarios…</p>}
          {usersError && <p className="error">{usersError}</p>}
          {!usersLoading && !usersError && tenantUsers.length === 0 && (
            <p className="muted">Sin usuarios.</p>
          )}
          {tenantUsers.length > 0 && (
            <table>
              <thead>
                <tr>
                  <th>Nombre</th>
                  <th>Email</th>
                  <th>Alta</th>
                </tr>
              </thead>
              <tbody>
                {tenantUsers.map((u) => (
                  <tr key={u.id}>
                    <td>{u.name}</td>
                    <td>{u.email}</td>
                    <td>{u.created_at}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}

          <h2>Crear admin</h2>
          <form onSubmit={onCreateAdmin}>
            <input
              type="email"
              required
              placeholder="email"
              value={aEmail}
              onChange={(e) => setAEmail(e.target.value)}
              autoComplete="email"
            />
            <input
              required
              placeholder="nombre"
              value={aName}
              onChange={(e) => setAName(e.target.value)}
              autoComplete="name"
            />
            <input
              type="password"
              required
              minLength={12}
              placeholder="clave (12+)"
              value={aPassword}
              onChange={(e) => setAPassword(e.target.value)}
              autoComplete="new-password"
            />
            <input
              type="password"
              required
              minLength={12}
              placeholder="confirmar clave"
              value={aConfirm}
              onChange={(e) => setAConfirm(e.target.value)}
              autoComplete="new-password"
            />
            <button type="submit" disabled={creatingAdmin}>
              {creatingAdmin ? "…" : "Crear admin"}
            </button>
          </form>
          {adminError && <p className="error">{adminError}</p>}
        </>
      )}
    </main>
  );
}
