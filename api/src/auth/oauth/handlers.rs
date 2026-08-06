use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    Json,
};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::Deserialize;
use serde_json::json;

use crate::{
    app::App,
    auth::handlers::issue_token_pair,
    database::models::{user_token, user_token_type::UserTokenType},
    token::{generate_secure_token, hash_token},
};

use super::{
    providers::{OauthProfile, OauthProvider},
    state::{sign_state, verify_state},
    upsert::upsert_oauth_user,
};

/// `GET /api/auth/oauth/providers` — which social buttons the app should show.
pub async fn oauth_providers<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
) -> impl IntoResponse
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let list: Vec<&'static str> = OauthProvider::all()
        .iter()
        .filter(|p| is_configured(&app, **p))
        .map(|p| p.as_str())
        .collect();
    Json(json!({ "providers": list }))
}

/// `GET /api/auth/oauth/{provider}/start`
pub async fn oauth_start<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    Path(provider): Path<String>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let Ok(provider) = provider.parse::<OauthProvider>() else {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "unknown_provider" }))).into_response();
    };
    if !is_configured(&app, provider) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "provider_not_configured" })),
        )
            .into_response();
    }

    let state = sign_state(&app.config.auth.secret, provider);
    let callback = format!(
        "{}/api/auth/oauth/{}/callback",
        app.config.api_url.trim_end_matches('/'),
        provider.as_str()
    );

    let url = match provider {
        OauthProvider::Google => {
            let client_id = app.config.auth.oauth.google.as_ref().unwrap().client_id.clone();
            format!(
                "https://accounts.google.com/o/oauth2/v2/auth?\
                 client_id={client_id}\
                 &redirect_uri={callback}\
                 &response_type=code\
                 &scope=openid%20email%20profile\
                 &state={state}\
                 &access_type=online\
                 &prompt=select_account"
            )
        }
        OauthProvider::Discord => {
            let client_id = app.config.auth.oauth.discord.as_ref().unwrap().client_id.clone();
            format!(
                "https://discord.com/api/oauth2/authorize?\
                 client_id={client_id}\
                 &redirect_uri={callback}\
                 &response_type=code\
                 &scope=identify%20email\
                 &state={state}"
            )
        }
        OauthProvider::Apple => {
            let client_id = app.config.auth.oauth.apple.as_ref().unwrap().client_id.clone();
            format!(
                "https://appleid.apple.com/auth/authorize?\
                 client_id={client_id}\
                 &redirect_uri={callback}\
                 &response_type=code\
                 &response_mode=query\
                 &scope=name%20email\
                 &state={state}"
            )
        }
    };

    // URL-encode redirect_uri in authorize URL — callback contains reserved chars.
    let url = url.replace(
        &format!("redirect_uri={callback}"),
        &format!(
            "redirect_uri={}",
            urlencoding_encode(&callback)
        ),
    );

    Redirect::temporary(&url).into_response()
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

/// `GET /api/auth/oauth/{provider}/callback`
pub async fn oauth_callback<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    Path(provider): Path<String>,
    Query(q): Query<CallbackQuery>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    if let Some(err) = q.error {
        return redirect_app_error(&app, &format!("oauth_error:{err}"));
    }

    let Ok(provider) = provider.parse::<OauthProvider>() else {
        return redirect_app_error(&app, "unknown_provider");
    };

    let Some(state_raw) = q.state.as_deref() else {
        return redirect_app_error(&app, "missing_state");
    };
    let state = match verify_state(&app.config.auth.secret, state_raw) {
        Ok(s) => s,
        Err(_) => return redirect_app_error(&app, "invalid_state"),
    };
    if state.provider != provider.as_str() {
        return redirect_app_error(&app, "state_provider_mismatch");
    }

    let Some(code) = q.code.as_deref() else {
        return redirect_app_error(&app, "missing_code");
    };

    let profile = match exchange_code(&app, provider, code).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("OAuth exchange failed for {provider}: {e}");
            return redirect_app_error(&app, "exchange_failed");
        }
    };

    let user = match upsert_oauth_user(&app, provider, &profile).await {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!("OAuth upsert failed: {e}");
            return redirect_app_error(&app, "upsert_failed");
        }
    };

    // One-time exchange code (short TTL).
    let raw = generate_secure_token(64);
    let expires_at = Utc::now().naive_utc() + chrono::Duration::minutes(5);
    let insert = user_token::ActiveModel {
        user_id: Set(user.id),
        token_type: Set(UserTokenType::OauthExchange),
        token_hash: Set(hash_token(&raw)),
        expires_at: Set(expires_at),
        ..Default::default()
    }
    .insert(&app.db)
    .await;
    if insert.is_err() {
        return redirect_app_error(&app, "token_issue_failed");
    }

    let dest = format!(
        "{}/oauth/callback?code={raw}&provider={}",
        app.config.app_url().trim_end_matches('/'),
        provider.as_str()
    );
    Redirect::temporary(&dest).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ExchangeBody {
    pub code: String,
}

/// `POST /api/auth/oauth/exchange` — trade one-time code for JWT pair.
pub async fn oauth_exchange<ExtraConfig>(
    State(app): State<App<ExtraConfig>>,
    Json(body): Json<ExchangeBody>,
) -> Response
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let token_hash = hash_token(&body.code);
    let now = Utc::now().naive_utc();

    let row = match user_token::Entity::find()
        .filter(user_token::Column::TokenHash.eq(&token_hash))
        .filter(user_token::Column::TokenType.eq(UserTokenType::OauthExchange))
        .one(&app.db)
        .await
    {
        Ok(Some(r)) if r.expires_at > now => r,
        Ok(Some(_)) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "code_expired" })),
            )
                .into_response()
        }
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_code" })),
            )
                .into_response()
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    // Consume the one-time code.
    let _ = user_token::Entity::delete_by_id(row.id).exec(&app.db).await;

    let user = match crate::database::models::user::Entity::find_by_id(row.user_id)
        .one(&app.db)
        .await
    {
        Ok(Some(u)) => u,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    match issue_token_pair(&app, &user).await {
        Ok(pair) => (StatusCode::OK, Json(pair)).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn is_configured<ExtraConfig>(app: &App<ExtraConfig>, provider: OauthProvider) -> bool {
    let o = &app.config.auth.oauth;
    match provider {
        OauthProvider::Google => o
            .google
            .as_ref()
            .is_some_and(|c| !c.client_id.is_empty() && !c.client_secret.is_empty()),
        OauthProvider::Discord => o
            .discord
            .as_ref()
            .is_some_and(|c| !c.client_id.is_empty() && !c.client_secret.is_empty()),
        OauthProvider::Apple => o.apple.as_ref().is_some_and(|c| {
            !c.client_id.is_empty()
                && !c.team_id.is_empty()
                && !c.key_id.is_empty()
                && !c.private_key_pem.is_empty()
        }),
    }
}

fn redirect_app_error<ExtraConfig>(app: &App<ExtraConfig>, error: &str) -> Response {
    let dest = format!(
        "{}/oauth/callback?error={}",
        app.config.app_url().trim_end_matches('/'),
        urlencoding_encode(error)
    );
    Redirect::temporary(&dest).into_response()
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

async fn exchange_code<ExtraConfig>(
    app: &App<ExtraConfig>,
    provider: OauthProvider,
    code: &str,
) -> Result<OauthProfile, String>
where
    ExtraConfig: Clone + Send + Sync + 'static,
{
    let callback = format!(
        "{}/api/auth/oauth/{}/callback",
        app.config.api_url.trim_end_matches('/'),
        provider.as_str()
    );
    let client = reqwest::Client::new();

    match provider {
        OauthProvider::Google => {
            let cfg = app.config.auth.oauth.google.as_ref().ok_or("no google")?;
            let token_res: serde_json::Value = client
                .post("https://oauth2.googleapis.com/token")
                .form(&[
                    ("code", code),
                    ("client_id", cfg.client_id.as_str()),
                    ("client_secret", cfg.client_secret.as_str()),
                    ("redirect_uri", callback.as_str()),
                    ("grant_type", "authorization_code"),
                ])
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;

            let access = token_res["access_token"]
                .as_str()
                .ok_or("missing access_token")?;
            let info: serde_json::Value = client
                .get("https://openidconnect.googleapis.com/v1/userinfo")
                .bearer_auth(access)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;

            Ok(OauthProfile {
                subject: info["sub"].as_str().unwrap_or("").to_string(),
                email: info["email"].as_str().unwrap_or("").to_string(),
                email_verified: info["email_verified"].as_bool().unwrap_or(true),
                display_name: info["name"].as_str().map(|s| s.to_string()),
            })
        }
        OauthProvider::Discord => {
            let cfg = app.config.auth.oauth.discord.as_ref().ok_or("no discord")?;
            let token_res: serde_json::Value = client
                .post("https://discord.com/api/oauth2/token")
                .form(&[
                    ("code", code),
                    ("client_id", cfg.client_id.as_str()),
                    ("client_secret", cfg.client_secret.as_str()),
                    ("redirect_uri", callback.as_str()),
                    ("grant_type", "authorization_code"),
                ])
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;

            let access = token_res["access_token"]
                .as_str()
                .ok_or("missing access_token")?;
            let info: serde_json::Value = client
                .get("https://discord.com/api/users/@me")
                .bearer_auth(access)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;

            let email = info["email"].as_str().unwrap_or("").to_string();
            Ok(OauthProfile {
                subject: info["id"].as_str().unwrap_or("").to_string(),
                email,
                email_verified: info["verified"].as_bool().unwrap_or(false),
                display_name: info["global_name"]
                    .as_str()
                    .or_else(|| info["username"].as_str())
                    .map(|s| s.to_string()),
            })
        }
        OauthProvider::Apple => {
            let cfg = app.config.auth.oauth.apple.as_ref().ok_or("no apple")?;
            let client_secret = apple_client_secret(cfg).map_err(|e| e.to_string())?;
            let token_res: serde_json::Value = client
                .post("https://appleid.apple.com/auth/token")
                .form(&[
                    ("code", code),
                    ("client_id", cfg.client_id.as_str()),
                    ("client_secret", client_secret.as_str()),
                    ("redirect_uri", callback.as_str()),
                    ("grant_type", "authorization_code"),
                ])
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;

            let id_token = token_res["id_token"]
                .as_str()
                .ok_or("missing id_token")?;
            // Decode JWT payload without full JWKS verify for first version;
            // signature verification via Apple JWKS is the hardening follow-up.
            let profile = decode_jwt_payload(id_token)?;
            let email = profile
                .get("email")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let sub = profile
                .get("sub")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let email_verified = profile
                .get("email_verified")
                .and_then(|v| v.as_bool().or_else(|| v.as_str().map(|s| s == "true")))
                .unwrap_or(true);
            Ok(OauthProfile {
                subject: sub,
                email,
                email_verified,
                display_name: None,
            })
        }
    }
}

fn apple_client_secret(cfg: &crate::config::AppleOauthConfig) -> Result<String, String> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use serde::Serialize;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Serialize)]
    struct Claims {
        iss: String,
        iat: u64,
        exp: u64,
        aud: String,
        sub: String,
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let claims = Claims {
        iss: cfg.team_id.clone(),
        iat: now,
        exp: now + 3600 * 24 * 150, // ~5 months, Apple max 6 months
        aud: "https://appleid.apple.com".into(),
        sub: cfg.client_id.clone(),
    };
    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some(cfg.key_id.clone());
    let key = EncodingKey::from_ec_pem(cfg.private_key_pem.as_bytes())
        .map_err(|e| format!("apple private key: {e}"))?;
    encode(&header, &claims, &key).map_err(|e| e.to_string())
}

fn decode_jwt_payload(jwt: &str) -> Result<serde_json::Value, String> {
    let parts: Vec<_> = jwt.split('.').collect();
    if parts.len() < 2 {
        return Err("malformed jwt".into());
    }
    let payload = base64::Engine::decode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        parts[1],
    )
    .or_else(|_| {
        base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE, parts[1])
    })
    .map_err(|e| e.to_string())?;
    serde_json::from_slice(&payload).map_err(|e| e.to_string())
}
