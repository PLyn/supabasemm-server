use axum::{
    extract::State,
    response::{Html, IntoResponse},
};
use crate::models::app_config::AppState;

pub async fn test_handler(State(_app_state): State<AppState>) -> impl IntoResponse {
    eprintln!("Hello world log!");
    return Html(format!("<h1>Hello World!</h1>"));
}