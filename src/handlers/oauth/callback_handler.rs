use crate::models::AppState;
use crate::models::oauth::{OAuthSessionData, CallbackParams};
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use oauth2::PkceCodeVerifier;
use serde::Deserialize;
use tower_sessions::Session;

pub async fn callback_handler(
    Query(params): Query<CallbackParams>,
    State(app_state): State<AppState>,
    session: Session,
) -> impl IntoResponse {
    eprintln!(
        "OAuth callback received. Code: {}, State: {}",
        params.code, params.state
    );

    let oauth_data: Option<OAuthSessionData> = match session.get("oauth_data").await {
        Ok(data) => data,
        Err(_) => None,
    };
    eprintln!(
        "Session ID: {:?} to get oauth retrieved from session: {:?}",
        session.id(),
        oauth_data
    );
    let oauth_data = match oauth_data {
        Some(data) => data,
        None => {
            eprintln!("No oauth_data found in session");
            let pkce_verifier = session
                .get::<String>("pkce_verifier_secret")
                .await
                .ok()
                .flatten();
            let csrf_token = session
                .get::<String>("csrf_token_secret")
                .await
                .ok()
                .flatten();

            if pkce_verifier.is_some() && csrf_token.is_some() {
                eprintln!("Found direct PKCE and CSRF keys instead");
                OAuthSessionData {
                    pkce_verifier_secret: pkce_verifier,
                    csrf_token_secret: csrf_token,
                }
            } else {
                return Html(
                    "<h1>Error</h1><p>No session data found. Please try logging in again.</p>\
                     <p><a href=\"/connect-supabase/login\">Back to Login</a></p>"
                        .to_string(),
                );
            }
        }
    };

    session.remove::<OAuthSessionData>("oauth_data").await.ok();

    if oauth_data.pkce_verifier_secret.is_none() {
        eprintln!("No PKCE verifier found in session");
        return Html(
            "<h1>Error</h1><p>No PKCE verifier found in session. Please try logging in again.</p>\
             <p><a href=\"/connect-supabase/login\">Back to Login</a></p>"
                .to_string(),
        );
    }
    let pkce_verifier_secret = oauth_data.pkce_verifier_secret.unwrap();

    if oauth_data.csrf_token_secret.is_none() {
        eprintln!("No CSRF token found in session");
        return Html(
            "<h1>Error</h1><p>No CSRF token found in session. Please try logging in again.</p>\
             <p><a href=\"/connect-supabase/login\">Back to Login</a></p>"
                .to_string(),
        );
    }
    let original_csrf_secret = oauth_data.csrf_token_secret.unwrap();

    if original_csrf_secret != params.state {
        eprintln!(
            "CSRF token mismatch. Expected: {}, Got: {}",
            original_csrf_secret, params.state
        );
        return Html(
            "<h1>Error</h1><p>CSRF token mismatch. Please try logging in again.</p>".to_string(),
        );
    }

    let pkce_verifier = PkceCodeVerifier::new(pkce_verifier_secret);

    let client = reqwest::Client::new();

    let params = [
        ("client_id", app_state.config.client_id.as_str()),
        ("client_secret", app_state.config.client_secret.as_str()),
        ("code", params.code.as_str()),
        ("code_verifier", pkce_verifier.secret()),
        ("grant_type", "authorization_code"),
        ("redirect_uri", app_state.config.redirect_url.as_str()),
    ];

    let response = match client.post("https://api.supabase.com/v1/oauth/token").form(&params).send().await {
        Ok(res) => res,
        Err(e) => {
            eprintln!("Failed to exchange token: {:?}", e);
            return Html(format!(
                "<h1>Error</h1><p>Failed to exchange token: {}. Please try logging in again.</p>",
                e
            ));
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Could not read error body".to_string());
        eprintln!("Failed to exchange token (HTTP {}): {}", status, error_text);
        return Html(format!(
            "<h1>Error</h1><p>Failed to exchange token: HTTP {} - {}. Please try logging in again.</p>",
            status, error_text
        ));
    }

    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
        refresh_token: Option<String>,
    }

    let token_data = match response.json::<TokenResponse>().await {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Failed to parse token response: {:?}", e);
            return Html(format!(
                "<h1>Error</h1><p>Failed to parse token response: {}. Please try logging in again.</p>",
                e
            ));
        }
    };

    session
        .insert("supabase_access_token", token_data.access_token.clone())
        .await
        .expect("Failed to store access token in session");

    if let Some(refresh_token) = token_data.refresh_token {
        eprintln!(
            "Refresh Token received (store securely if needed for long-term use): {}",
            refresh_token
        );
    }

    Html(format!(
        r#"
        <!DOCTYPE html>
        <html>
        <head>
            <meta http-equiv="refresh" content="0;url=/migrate">
            <title>Redirecting...</title>
        </head>
        <body>
            <p>Authentication successful! Redirecting to your projects...</p>
            <p>If you are not redirected, <a href="/migrate">click here</a>.</p>
        </body>
        </html>
        "#
    ))
}
