/**
 * Cliente fetch tipado contra el API del backend OCCB.
 *
 * - Base URL desde `NEXT_PUBLIC_API_URL` (sin `/` final). Todo cuelga de `/api/v1`.
 * - Errores del backend: `{ error: { code, message } }` (+ `Retry-After` en 429).
 * - Este módulo NO guarda tokens: la autenticación vive en memoria React
 *   (ver `lib/auth.tsx`), nunca en localStorage/sessionStorage.
 */

export interface UserPublic {
  id: string;
  email: string;
  name: string;
  created_at: string;
}

export interface TokenPair {
  user: UserPublic;
  access_token: string;
  refresh_token: string;
  token_type: string;
  expires_in: number;
}

export interface UsersPage {
  data: UserPublic[];
  page: number;
  per_page: number;
  total: number;
}

export interface ApiErrorBody {
  error: {
    code: string;
    message: string;
  };
}

/** Error tipado de la API: conserva `status` HTTP y `code` de negocio. */
export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly retryAfter: string | null;

  constructor(status: number, code: string, message: string, retryAfter: string | null = null) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
    this.retryAfter = retryAfter;
  }
}

export function getApiBaseUrl(): string {
  const raw = process.env.NEXT_PUBLIC_API_URL ?? "http://127.0.0.1:8000";
  return raw.replace(/\/+$/, "");
}

async function parseFailure(res: Response): Promise<ApiError> {
  let code = "unknown";
  let message = `HTTP ${res.status}`;
  try {
    const body = (await res.json()) as Partial<ApiErrorBody>;
    if (body?.error) {
      code = body.error.code ?? code;
      message = body.error.message ?? message;
    }
  } catch {
    // Cuerpo no-JSON (p. ej. 413/408 del borde): mensaje genérico por estado.
  }
  const retryAfter = res.status === 429 ? res.headers.get("Retry-After") : null;
  return new ApiError(res.status, code, message, retryAfter);
}

export interface RequestOptions {
  method?: string;
  /** Cuerpo serializado a JSON. */
  body?: unknown;
  /** Access token (se envía como `Authorization: Bearer ...`). */
  token?: string | null;
  /** Permite reintentar tras refresh sin re-serializar desde fuera. */
  headers?: HeadersInit;
}

async function doFetch(path: string, opts: RequestOptions): Promise<Response> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (opts.token) headers["Authorization"] = `Bearer ${opts.token}`;
  if (opts.headers) {
    const extra = new Headers(opts.headers);
    extra.forEach((v, k) => {
      headers[k] = v;
    });
  }
  return fetch(`${getApiBaseUrl()}${path}`, {
    method: opts.method ?? "GET",
    headers,
    body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
  });
}

/** Petición que devuelve JSON tipado o lanza `ApiError`. */
export async function requestJson<T>(path: string, opts: RequestOptions = {}): Promise<T> {
  const res = await doFetch(path, opts);
  if (!res.ok) throw await parseFailure(res);
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

/** GET tipado. */
export function apiGet<T>(path: string, opts: Omit<RequestOptions, "method" | "body"> = {}): Promise<T> {
  return requestJson<T>(path, { ...opts, method: "GET" });
}

/** POST tipado con cuerpo JSON. */
export function apiPost<T>(
  path: string,
  body: unknown,
  opts: Omit<RequestOptions, "method" | "body"> = {},
): Promise<T> {
  return requestJson<T>(path, { ...opts, method: "POST", body });
}

/**
 * POST sin cuerpo JSON (p. ej. logout con 204). Devuelve la `Response`
 * cruda para que el llamador inspeccione el estado.
 */
export async function apiPostRaw(
  path: string,
  body: unknown,
  opts: Omit<RequestOptions, "method" | "body"> = {},
): Promise<Response> {
  const res = await doFetch(path, { ...opts, method: "POST", body });
  if (!res.ok) throw await parseFailure(res);
  return res;
}
