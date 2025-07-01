mod models;
mod handlers;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use axum::{routing::get, Router};
    use models::{AppConfig, AppState};
    use handlers::test_handler;
    use handlers::migrate::preview_handler;
    use tower_sessions::{Expiry, MemoryStore, SessionManagerLayer};
    use time::Duration;
    
    //use handlers::{callback_handler, login_handler};

    let app_config = AppConfig::from_env()?;

    let app_state = AppState {
        config: app_config.clone(),
    };

    let session_store = MemoryStore::default();
    let session_expiry = Expiry::OnInactivity(Duration::hours(6));
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_same_site(tower_sessions::cookie::SameSite::Lax)
        .with_expiry(session_expiry);

    let app = Router::new()
        .route("/", get(test_handler))
        .route("/preview", get(preview_handler))
        //.route("/connect-supabase/login", get(login_handler))
        //.route("/connect-supabase/oauth2/callback", get(callback_handler))
        .layer(session_layer)
        .with_state(app_state);

    eprintln!("listening on http://{}", "0.0.0.0:10000");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:10000").await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}