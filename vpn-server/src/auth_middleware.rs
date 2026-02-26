use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use jsonwebtoken::{decode, DecodingKey, Validation};

use crate::models::Claims;
use crate::state::Shared;

// ── Extractors ────────────────────────────────────────────────────────────────

/// Any authenticated user.
pub struct AuthUser(pub Claims);

/// Admin-only (role == "admin").
pub struct AdminUser(pub Claims);

fn extract_bearer(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

fn decode_claims(token: &str, secret: &str) -> Option<Claims> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .ok()
    .map(|td| td.claims)
}

#[axum::async_trait]
impl FromRequestParts<Shared> for AuthUser {
    type Rejection = (StatusCode, String);

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
