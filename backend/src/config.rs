//! Configuración desde variables de entorno con validación fail-fast.
//!
//! Reglas de seguridad:
//! - `DATABASE_URL` y `JWT_SECRET` son obligatorios; el arranque falla si faltan.
//! - `JWT_SECRET` >= 32 caracteres; en prod se rechazan valores de ejemplo.
//! - `CORS_ORIGINS` es lista cerrada (nunca `*` con credenciales).
//! - Los secretos JAMÁS se escriben en logs (ver `database_url_redacted`).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEnv {
    Development,
    Production,
}

impl AppEnv {
    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "prod" | "production" => Self::Production,
            _ => Self::Development,
        }
    }

    pub fn is_prod(self) -> bool {
        matches!(self, Self::Production)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Production => "production",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub env: AppEnv,
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub redis_url: Option<String>,
    pub jwt_secret: String,
    pub jwt_issuer: String,
    pub jwt_audience: String,
    pub access_token_ttl_minutes: i64,
    pub refresh_token_ttl_days: i64,
    pub cors_origins: Vec<String>,
    pub frontend_url: String,
    pub body_limit_bytes: usize,
    pub rate_limit_per_minute: u32,
    /// true solo si hay UN proxy de confianza delante (Caddy). Afecta a la
    /// extracción de IP cliente para rate-limit. Por defecto false (seguro).
    pub trusted_proxy: bool,
}

fn required(name: &str) -> String {
    std::env::var(name)
        .unwrap_or_else(|_| panic!("{name} debe estar definida (ver .env.*.example)"))
}

fn optional(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn parse_u16(name: &str, default: u16) -> u16 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn parse_i64(name: &str, default: i64) -> i64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

impl Config {
    pub fn from_env() -> Self {
        // En local carga .env.dev si existe; en docker las vars ya vienen del env_file.
        let _ = dotenvy::from_filename(".env.dev");
        let _ = dotenvy::dotenv();

        let env = AppEnv::from_str(&optional("APP_ENV", "development"));

        let database_url = required("DATABASE_URL");
        let jwt_secret = required("JWT_SECRET");
        if jwt_secret.len() < 32 {
            panic!(
                "JWT_SECRET debe tener al menos 32 caracteres (tiene {}). Genera con: openssl rand -base64 48",
                jwt_secret.len()
            );
        }
        if env.is_prod() {
            let low = jwt_secret.to_ascii_lowercase();
            for bad in [
                "changeme",
                "change-me",
                "secret",
                "password",
                "test",
                "example",
            ] {
                if low.contains(bad) {
                    panic!("JWT_SECRET parece un valor de ejemplo; genera uno real para prod");
                }
            }
            if database_url.contains("changeme") || database_url.contains("OBLIGATORIO") {
                panic!("DATABASE_URL parece de ejemplo; revisa credenciales de prod");
            }
        }

        let cors_origins: Vec<String> = optional("CORS_ORIGINS", "http://localhost:3000")
            .split(',')
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if cors_origins.is_empty() {
            panic!("CORS_ORIGINS no puede estar vacía");
        }
        for o in &cors_origins {
            if !(o.starts_with("http://") || o.starts_with("https://")) {
                panic!("CORS_ORIGINS inválido: {o} (debe ser http(s)://...)");
            }
            if env.is_prod() && o.starts_with("http://") {
                tracing::warn!("CORS_ORIGINS con http:// (sin TLS) en prod: {o}");
            }
        }

        let body_limit_mb: usize = std::env::var("BODY_LIMIT_MB")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);

        Self {
            env,
            host: optional("SERVER_HOST", "0.0.0.0"),
            port: parse_u16("SERVER_PORT", 8000),
            database_url,
            redis_url: std::env::var("REDIS_URL").ok().filter(|s| !s.is_empty()),
            jwt_secret,
            jwt_issuer: optional("JWT_ISSUER", "occb"),
            jwt_audience: optional("JWT_AUDIENCE", "occb-frontend"),
            access_token_ttl_minutes: parse_i64("ACCESS_TOKEN_TTL_MINUTES", 15),
            refresh_token_ttl_days: parse_i64("REFRESH_TOKEN_TTL_DAYS", 30),
            cors_origins,
            frontend_url: optional("FRONTEND_URL", "http://localhost:3000"),
            body_limit_bytes: body_limit_mb.saturating_mul(1024 * 1024),
            rate_limit_per_minute: std::env::var("RATE_LIMIT_PER_MINUTE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(120),
            trusted_proxy: std::env::var("TRUSTED_PROXY")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
        }
    }

    /// URL con la contraseña oculta, apta para logs.
    pub fn database_url_redacted(&self) -> String {
        if let Some(scheme) = self.database_url.find("://") {
            let after_scheme = &self.database_url[scheme + 3..];
            if let Some(at) = self.database_url.rfind('@') {
                let host_part = &self.database_url[at + 1..];
                let userinfo =
                    &after_scheme[..after_scheme.len().saturating_sub(host_part.len() + 1)];
                let user = userinfo.split(':').next().unwrap_or("");
                return format!(
                    "{}://{}:***@{}",
                    &self.database_url[..scheme],
                    user,
                    host_part
                );
            }
        }
        self.database_url.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacted_oculta_password() {
        let cfg = Config {
            env: AppEnv::Development,
            host: "0.0.0.0".into(),
            port: 8000,
            database_url: "postgres://occb_app:super-secreto@db:5432/occb_db".into(),
            redis_url: None,
            jwt_secret: "x".repeat(32),
            jwt_issuer: "occb".into(),
            jwt_audience: "occb-frontend".into(),
            access_token_ttl_minutes: 15,
            refresh_token_ttl_days: 30,
            cors_origins: vec!["http://localhost:3000".into()],
            frontend_url: "http://localhost:3000".into(),
            body_limit_bytes: 1024 * 1024,
            rate_limit_per_minute: 120,
            trusted_proxy: true,
        };
        let r = cfg.database_url_redacted();
        assert!(!r.contains("super-secreto"), "fuga de password: {r}");
        assert!(r.contains("occb_app"));
        assert!(r.contains("db:5432/occb_db"));
    }
}
