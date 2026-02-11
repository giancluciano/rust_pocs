use actix_web::{web, App, HttpServer, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

#[derive(Deserialize)]
struct FreightRequest {
    volume: f64,
    size: f64,
    type_transport: String,
}

#[derive(Serialize)]
struct PriceResponse {
    price: f64,
}

fn compute_price(base_price: f64, volume: f64, size: f64) -> f64 {
    base_price + (volume * size)
}

async fn calculate_price(
    pool: web::Data<SqlitePool>,
    req: web::Json<FreightRequest>,
) -> HttpResponse {
    let result = sqlx::query_scalar::<_, f64>(
        "SELECT base_price FROM freight_price WHERE type_transport = ?",
    )
    .bind(&req.type_transport)
    .fetch_optional(pool.get_ref())
    .await;

    match result {
        Ok(Some(base_price)) => {
            let price = compute_price(base_price, req.volume, req.size);
            HttpResponse::Ok().json(PriceResponse { price })
        }
        Ok(None) => HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("transport type '{}' not found", req.type_transport)
        })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "error": e.to_string()
        })),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let pool = SqlitePoolOptions::new()
        .connect("database.db")
        .await
        .expect("failed to connect to database");

    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("failed to run migrations");

    println!("Server running at http://localhost:8080");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .route("/calculate_price", web::post().to(calculate_price))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_price() {
        let price = compute_price(100.0, 2.0, 3.0);
        assert_eq!(price, 106.0);
    }
}
