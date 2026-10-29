//! Autorización RBAC por perfiles y permisos.
//!
//! - `require_permission("permiso")`: tras el auth JWT, carga los permisos
//!   del perfil del usuario y permite solo si incluye el requerido.
//! - `admin.access` lo permite todo (convención).
//! - Denegaciones: 403 + warn en log con usuario y permiso.

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

use crate::{auth::Claims, error::AppError, state::AppState};

#[derive(Debug, Clone)]
pub struct UserPermissions(pub Vec<String>);

/// Lógica pura y testeable.
pub fn permitted(user_permissions: &[String], required: &str) -> bool {
    user_permissions
        .iter()
        .any(|p| p == required || p == "admin.access")
}

pub async fn require_permission(
    State((state, permission)): State<(AppState, &'static str)>,
    mut req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or(AppError::Unauthorized)?;
    let permissions: Vec<String> = sqlx::query_scalar(
        "SELECT pp.permission FROM profile_permissions pp \
         JOIN users u ON u.profile_id = pp.profile_id \
         WHERE u.id = $1",
    )
    .bind(claims.sub)
    .fetch_all(&state.db)
    .await?;
    if permitted(&permissions, permission) {
        req.extensions_mut().insert(UserPermissions(permissions));
        Ok(next.run(req).await)
    } else {
        tracing::warn!(user = %claims.sub, %permission, "acceso denegado por RBAC");
        Err(AppError::Forbidden)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perms(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn admin_access_lo_permite_todo() {
        assert!(permitted(&perms(&["admin.access"]), "cualquier.cosa"));
        assert!(permitted(&perms(&["admin.access"]), "admin.access"));
    }

    #[test]
    fn permiso_exact_o_denegado() {
        assert!(permitted(&perms(&["users.read"]), "users.read"));
        assert!(!permitted(&perms(&["users.read"]), "users.write"));
        assert!(!permitted(&perms(&[]), "users.read"));
    }
}
