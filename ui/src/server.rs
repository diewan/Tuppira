/// Simple static file server for Dioxus web UI

use axum::{
    routing::get,
    Router,
    response::{Html, IntoResponse},
    http::StatusCode,
};
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    let port = std::env::var("UI_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let api_url = std::env::var("API_URL")
        .unwrap_or_else(|_| "http://localhost:8080".to_string());

    let addr: std::net::SocketAddr = ([0, 0, 0, 0], port).into();

    println!("Starting CSV Explorer UI on http://0.0.0.0:{}", port);
    println!("API URL: {}", api_url);
    println!("");

    // Build the app with SSR support
    let app = Router::new()
        .route("/", get(index_html))
        .nest_service("/assets", ServeDir::new("ui/assets").not_found_service(get(not_found)))
        .fallback(get(not_found));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind address");

    println!("Server ready, accepting connections on {}", addr);

    axum::serve(listener, app)
        .await
        .expect("Server error");
}

async fn index_html() -> impl IntoResponse {
    Html(INDEX_HTML)
}

async fn not_found() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "Not Found")
}

const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>CSV Explorer</title>
    <link rel="stylesheet" href="/assets/tailwind.css">
    <link rel="stylesheet" href="/assets/styles.css">
</head>
<body>
    <div id="main"></div>
    <script type="module">
        import init from '/assets/dioxus/web/dioxus.js';
        init().then(() => {
            console.log('Dioxus UI loaded');
        });
    </script>
</body>
</html>
"#;
