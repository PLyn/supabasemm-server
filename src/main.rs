mod models;
mod handlers;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use axum::{routing::get, Router};
    use models::{AppConfig, AppState};
    use handlers::test_handler;
    
    //use handlers::{callback_handler, login_handler};

    let app_config = AppConfig::from_env()?;

    let app_state = AppState {
        config: app_config.clone(),
    };

    let app = Router::new()
        .route("/", get(test_handler))
        //.route("/connect-supabase/login", get(login_handler))
        //.route("/connect-supabase/oauth2/callback", get(callback_handler))
        .with_state(app_state);

    eprintln!("listening on http://{}", "0.0.0.0:10000");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:10000").await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}