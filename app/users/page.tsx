"use client";

/**
 * Listado paginado de usuarios (GET /api/v1/users) + detalle por id
 * (GET /api/v1/users/{id}). Requiere sesión (usa `apiFetch` autenticado).
 */

import { useCallback, useEffect, useState } from "react";
import Link from "next/link";
import { useAuth } from "../../lib/auth";
import { ApiError } from "../../lib/api";
import type { UserPublic, UsersPage } from "../../lib/api";

export default function UsersPage() {
  const { user, apiFetch } = useAuth();
  const [q, setQ] = useState("");
  const [page, setPage] = useState(1);
  const [perPage, setPerPage] = useState(20);
  const [sort, setSort] = useState("created_at");
  const [dir, setDir] = useState("desc");
  const [data, setData] = useState<UsersPage | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Detalle por id.
  const [detailId, setDetailId] = useState("");
  const [detail, setDetail] = useState<UserPublic | null>(null);
  const [detailError, setDetailError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!user) return;
    setLoading(true);
    setError(null);
    try {
      const params = new URLSearchParams({
        page: String(page),
        per_page: String(perPage),
        sort,
        dir,
      });
      if (q.trim()) params.set("q", q.trim());
      const res = await apiFetch<UsersPage>(`/api/v1/users?${params.toString()}`);
      setData(res);
    } catch (e) {
      setError(e instanceof ApiError ? `${e.code}: ${e.message}` : "error de red");
    } finally {
      setLoading(false);
    }
  }, [user, apiFetch, q, page, perPage, sort, dir]);

  useEffect(() => {
    void load();
  }, [load]);

  async function loadDetail(e: React.FormEvent) {
    e.preventDefault();
    setDetail(null);
    setDetailError(null);
    const id = detailId.trim();
    if (!id) return;
    try {
      const u = await apiFetch<UserPublic>(`/api/v1/users/${encodeURIComponent(id)}`);
      setDetail(u);
    } catch (err) {
      setDetailError(err instanceof ApiError ? `${err.code}: ${err.message}` : "error de red");
    }
  }

  if (!user) {
    return (
      <main>
        <h1>Usuarios</h1>
        <p>
          Necesitas sesión. <Link href="/">Ir al login</Link>
        </p>
      </main>
    );
  }

  const totalPages = data ? Math.max(1, Math.ceil(data.total / data.per_page)) : 1;

  return (
    <main>
      <h1>Usuarios</h1>
      <nav>
        <Link href="/">Perfil</Link>
      </nav>

      <form
        onSubmit={(e) => {
          e.preventDefault();
          setPage(1);
          void load();
        }}
      >
        <div className="row">
          <input placeholder="buscar (q)" value={q} onChange={(e) => setQ(e.target.value)} />
          <select value={perPage} onChange={(e) => setPerPage(Number(e.target.value))}>
            {[10, 20, 50].map((n) => (
              <option key={n} value={n}>
                {n}/pág
              </option>
            ))}
          </select>
          <select value={sort} onChange={(e) => setSort(e.target.value)}>
            {["created_at", "email", "name"].map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
          <select value={dir} onChange={(e) => setDir(e.target.value)}>
            <option value="desc">desc</option>
            <option value="asc">asc</option>
          </select>
          <button type="submit">Buscar</button>
        </div>
      </form>

      {loading && <p className="muted">Cargando…</p>}
      {error && <p className="error">{error}</p>}
      {data && (
        <>
          <p className="muted">
            Página {data.page} de {totalPages} · {data.total} total
          </p>
          <table>
            <thead>
              <tr>
                <th>Nombre</th>
                <th>Email</th>
                <th>Alta</th>
              </tr>
            </thead>
            <tbody>
              {data.data.map((u) => (
                <tr key={u.id}>
                  <td>{u.name}</td>
                  <td>{u.email}</td>
                  <td>{u.created_at}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="row">
            <button type="button" disabled={page <= 1} onClick={() => setPage((p) => p - 1)}>
              ← Anterior
            </button>
            <button
              type="button"
              disabled={page >= totalPages}
              onClick={() => setPage((p) => p + 1)}
            >
              Siguiente →
            </button>
          </div>
        </>
      )}

      <h2>Detalle por id</h2>
      <form onSubmit={loadDetail}>
        <div className="row">
          <input
            placeholder="UUID de usuario"
            value={detailId}
            onChange={(e) => setDetailId(e.target.value)}
            style={{ minWidth: "18rem" }}
          />
          <button type="submit">Ver</button>
        </div>
      </form>
      {detailError && <p className="error">{detailError}</p>}
      {detail && (
        <div className="card">
          <p>
            <strong>{detail.name}</strong> · {detail.email}
          </p>
          <p className="muted">
            {detail.id} · alta {detail.created_at}
          </p>
        </div>
      )}
    </main>
  );
}
