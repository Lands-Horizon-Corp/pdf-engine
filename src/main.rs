mod handlers;
mod models;
mod routes;
mod services;
mod state;

use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    // 1. Initialize the in-memory state
    let app_state = state::AppState::new();

    // 2. Build our application router
    let app = routes::create_router(app_state);

    // 3. Start the server
    let listener = TcpListener::bind("127.0.0.1:3000").await.unwrap();
    println!("Listening on {}", listener.local_addr().unwrap());

    axum::serve(listener, app).await.unwrap();
}
