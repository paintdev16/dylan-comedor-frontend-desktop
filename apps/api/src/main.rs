use shared::{dto::HealthResponse, endpoints};

fn main() {
    let health = HealthResponse {
        service: "dylan-api".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        status: "ready".into(),
    };
    println!(
        "Backend {} v{} listo ({}) — pendiente de integrar Axum.",
        health.service,
        health.version,
        endpoints::HEALTH
    );
}
