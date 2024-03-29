#![recursion_limit = "256"]

mod config;
mod endpoints;
mod models;
mod resolving;
mod tax;
mod utils;
use axum::{http::StatusCode, Router};
use axum_auto_routes::route;
use mongodb::{bson::doc, options::ClientOptions, Client};
use std::sync::Arc;
use std::{net::SocketAddr, sync::Mutex};
use utils::WithState;

use tower_http::cors::{Any, CorsLayer};

lazy_static::lazy_static! {
    pub static ref ROUTE_REGISTRY: Mutex<Vec<Box<dyn WithState>>> = Mutex::new(Vec::new());
}

#[tokio::main]
async fn main() {
    println!("starknetid_server: starting v{}", env!("CARGO_PKG_VERSION"));
    let conf = config::load();

    let starknetid_client_options =
        ClientOptions::parse(&conf.databases.starknetid.connection_string)
            .await
            .unwrap();

    let sales_client_options = ClientOptions::parse(&conf.databases.sales.connection_string)
        .await
        .unwrap();

    let states = tax::sales_tax::load_sales_tax().await;
    if states.states.is_empty() {
        println!("error: unable to load sales tax");
        return;
    }

    let shared_state = Arc::new(models::AppState {
        conf: conf.clone(),
        starknetid_db: Client::with_options(starknetid_client_options)
            .unwrap()
            .database(&conf.databases.starknetid.name),
        sales_db: Client::with_options(sales_client_options)
            .unwrap()
            .database(&conf.databases.sales.name),
        states,
    });
    // we will know by looking at the log number which db has an issue
    for db in [&shared_state.starknetid_db, &shared_state.sales_db] {
        if db.run_command(doc! {"ping": 1}, None).await.is_err() {
            println!("error: unable to connect to a database");
            return;
        } else {
            println!("database: connected")
        }
    }

    let cors = CorsLayer::new().allow_headers(Any).allow_origin(Any);
    let app = ROUTE_REGISTRY
        .lock()
        .unwrap()
        .clone()
        .into_iter()
        .fold(Router::new().with_state(shared_state.clone()), |acc, r| {
            acc.merge(r.to_router(shared_state.clone()))
        })
        .layer(cors);

    let addr = SocketAddr::from(([0, 0, 0, 0], conf.server.port));
    println!("server: listening on http://0.0.0.0:{}", conf.server.port);
    axum::Server::bind(&addr)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .unwrap();
}

#[route(get, "/")]
async fn root() -> (StatusCode, String) {
    (
        StatusCode::ACCEPTED,
        format!("starknetid_server v{}", env!("CARGO_PKG_VERSION")),
    )
}
