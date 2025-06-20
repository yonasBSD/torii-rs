use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::get,
};
use axum_extra::extract::{CookieJar, cookie::Cookie};
use serde::Deserialize;
use sqlx::{Pool, Sqlite};
use torii_auth_oauth::OAuthPlugin;
use torii_core::{DefaultUserManager, Session, plugin::PluginManager, storage::SessionStorage};
use torii_storage_sqlite::SqliteStorage;

#[derive(Debug, Deserialize)]
struct QueryParams {
    code: String,
    state: String,
}

#[derive(Clone)]
struct AppState {
    plugin_manager: Arc<PluginManager<SqliteStorage, SqliteStorage>>,
}

#[axum::debug_handler]
async fn login_handler(State(state): State<AppState>, jar: CookieJar) -> (CookieJar, Redirect) {
    let plugin = state
        .plugin_manager
        .get_plugin::<OAuthPlugin<DefaultUserManager<SqliteStorage>, SqliteStorage>>("github")
        .unwrap();

    let auth_url = plugin.get_authorization_url().await.unwrap();

    let jar = jar.add(
        Cookie::build(("csrf_state", auth_url.csrf_state().to_string()))
            .path("/")
            .http_only(true),
    );

    (jar, Redirect::to(auth_url.url()))
}

#[axum::debug_handler]
async fn callback_handler(
    State(state): State<AppState>,
    Query(params): Query<QueryParams>,
    jar: CookieJar,
) -> impl IntoResponse {
    let csrf_state = jar.get("csrf_state").unwrap().value();

    if csrf_state != params.state {
        return (StatusCode::BAD_REQUEST, "CSRF state mismatch").into_response();
    }

    let plugin = state
        .plugin_manager
        .get_plugin::<OAuthPlugin<DefaultUserManager<SqliteStorage>, SqliteStorage>>("github")
        .unwrap();

    let (user, _) = plugin
        .exchange_code(params.code.to_string(), csrf_state.to_string())
        .await
        .unwrap();

    let session = state
        .plugin_manager
        .session_storage()
        .create_session(
            &Session::builder()
                .user_id(user.id.clone())
                .build()
                .expect("Failed to build session"),
        )
        .await
        .expect("Failed to create session");

    // Set session cookie
    let jar = jar.add(
        Cookie::build(("session_id", session.token.to_string()))
            .path("/")
            .http_only(true),
    );

    (jar, Json(user)).into_response()
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let pool = Pool::<Sqlite>::connect("sqlite:./github.db?mode=rwc")
        .await
        .unwrap();

    let storage = Arc::new(SqliteStorage::new(pool.clone()));
    let session_storage = Arc::new(SqliteStorage::new(pool.clone()));

    storage.migrate().await.unwrap();
    session_storage.migrate().await.unwrap();

    // Create user manager
    let user_manager = Arc::new(DefaultUserManager::new(storage.clone()));

    let mut plugin_manager = PluginManager::new(storage.clone(), session_storage.clone());
    plugin_manager.register_plugin(OAuthPlugin::github(
        &std::env::var("GITHUB_CLIENT_ID").expect("GITHUB_CLIENT_ID must be set"),
        &std::env::var("GITHUB_CLIENT_SECRET").expect("GITHUB_CLIENT_SECRET must be set"),
        "http://localhost:4000/auth/github/callback",
        user_manager.clone(),
        storage.clone(),
    ));

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/auth/github/login", get(login_handler))
        .route("/auth/github/callback", get(callback_handler))
        .with_state(AppState {
            plugin_manager: Arc::new(plugin_manager),
        });

    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind("0.0.0.0:4000").await.unwrap();
        println!("Listening on {}", listener.local_addr().unwrap());
        axum::serve(listener, app).await.unwrap();
    });

    println!(
        "Please open the following URL in your browser: http://localhost:4000/auth/github/login"
    );

    println!("Press Enter or Ctrl+C to exit...");
    let _ = std::io::stdin().read_line(&mut String::new());
}
