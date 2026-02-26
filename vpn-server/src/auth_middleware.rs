//! JWT-based authentication extractors for axum.
//!
//! Two extractors are provided:
//! * [`AuthUser`] — accepts any valid JWT (role `"user"` or `"admin"`).
//! * [`AdminUser`] — only accepts JWTs with role `"admin"`.
//!
//! ## Usage in handlers
//! ```rust,ignore
//! // Any authenticated user
//! async fn my_handler(AuthUser(claims): AuthUser, ...) { ... }
//!
//! // Admin only
//! async fn admin_handler(AdminUser(claims): AdminUser, ...) { ... }
//! ```
//!
//! Both extractors read the `Authorization: Bearer <token>` header, verify
//! the token's HMAC signature and expiry using the server's `jwt_secret`,
//! and return the decoded [`Claims`] payload on success.

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use jsonwebtoken::{decode, DecodingKey, Validation};

use crate::models::Claims;
use crate::state::Shared;

// ── Extractor types ───────────────────────────────────────────────────────────

/// Axum extractor — any authenticated user (role `"user"` or `"admin"`).
///
/// Fails with `401 Unauthorized` if the token is missing, malformed, expired
/// or signed with a different secret.
pub struct AuthUser(pub Claims);

/// Axum extractor — admin-only access (role must equal `"admin"`).
///
/// Fails with `401 Unauthorized` for a bad/missing token, or
/// `403 Forbidden` if the token is valid but has role `"user"`.
pub struct AdminUser(pub Claims);

// ── Helper: extract Bearer token from the Authorization header ────────────────

/// Extract the raw token string from an `Authorization: Bearer <token>` header.
///
/// Returns `None` if the header is absent, not valid UTF-8, or does not use
/// the `Bearer` scheme.
fn extract_bearer(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

/// Verify a JWT string and decode its [`Claims`] payload.
///
/// Returns `None` if the signature is invalid, the token is expired, or
/// decoding fails for any other reason.
fn decode_claims(token: &str, secret: &str) -> Option<Claims> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .ok()
    .map(|td| td.claims)
}

// ── FromRequestParts implementations ─────────────────────────────────────────

#[axum::async_trait]
impl FromRequestParts<Shared> for AuthUser {
    type Rejection = (StatusCode, String);

    /// Extract and validate the JWT, accepting any role.
    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, Self::Rejection> {
        let token = extract_bearer(parts)
            .ok_or((StatusCode::UNAUTHORIZED, "Missing Bearer token".into()))?;
        let claims = decode_claims(&token, &state.jwt_secret)
            .ok_or((StatusCode::UNAUTHORIZED, "Invalid or expired token".into()))?;
        Ok(AuthUser(claims))
    }
}

#[axum::async_trait]
impl FromRequestParts<Shared> for AdminUser {
    type Rejection = (StatusCode, String);

    /// Extract and validate the JWT, then assert `role == "admin"`.
    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, Self::Rejection> {
        let token = extract_bearer(parts)
            .ok_or((StatusCode::UNAUTHORIZED, "Missing Bearer token".into()))?;
        let claims = decode_claims(&token, &state.jwt_secret)
            .ok_or((StatusCode::UNAUTHORIZED, "Invalid or expired token".into()))?;
        if claims.role != "admin" {
            return Err((StatusCode::FORBIDDEN, "Admin only".into()));
        }
        Ok(AdminUser(claims))
    }
}

// ── JWT creation ──────────────────────────────────────────────────────────────

/// Issue a signed JWT for the given user.
///
/// The token carries the user's `id` (as `sub`) and `role`.  It is valid
/// for **30 days** from the moment of issuance.
///
/// # Panics
/// Panics if the JWT encoding fails (should never happen with a valid secret).
pub fn make_token(user_id: i32, role: &str, secret: &str) -> String {
    use jsonwebtoken::{encode, EncodingKey, Header};
    let exp = (chrono::Utc::now() + chrono::Duration::days(30)).timestamp() as usize;
    let claims = Claims {
        sub: user_id,
        role: role.to_string(),
        exp,
    };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes()))
        .expect("JWT encode failed")
}
