use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use diesel::prelude::*;
use dotenvy::dotenv;
use std::env;
use std::sync::{Arc, Mutex};

pub mod models;
pub mod schema;

use self::models::*;

type DbPool = Arc<Mutex<SqliteConnection>>;

pub fn establish_connection() -> SqliteConnection {
    dotenv().ok();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    SqliteConnection::establish(&database_url)
        .unwrap_or_else(|_| panic!("Error connecting to {}", database_url))
}

pub fn create_product(conn: &mut SqliteConnection, product_name: &str, product_value: &i32) -> Product {
    use crate::schema::products;

    let new_product = NewProduct { product_name, product_value };

    diesel::insert_into(products::table)
        .values(&new_product)
        .returning(Product::as_returning())
        .get_result(conn)
        .expect("Error saving new product")
}

pub fn create_tax(conn: &mut SqliteConnection, state_name: &str, year: &i32, percent: &i32, product_id: &i32) -> Tax {
    use crate::schema::taxes;

    let new_tax = NewTax { state_name, year, percent, product_id };

    diesel::insert_into(taxes::table)
        .values(&new_tax)
        .returning(Tax::as_returning())
        .get_result(conn)
        .expect("Error saving new tax")
}

pub fn get_product_by_id(conn: &mut SqliteConnection, product_id: i32) -> Option<Product> {
    use crate::schema::products::dsl::*;

    products
        .filter(id.eq(product_id))
        .select(Product::as_select())
        .first(conn)
        .optional()
        .expect("Error loading product")
}

// API Handlers
async fn get_product_handler(
    State(pool): State<DbPool>,
    Path(product_id): Path<i32>,
) -> Result<Json<Product>, StatusCode> {
    let mut conn = pool.lock().unwrap();
    match get_product_by_id(&mut conn, product_id) {
        Some(product) => Ok(Json(product)),
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn create_product_handler(
    State(pool): State<DbPool>,
    Json(payload): Json<CreateProductRequest>,
) -> Result<(StatusCode, Json<Product>), StatusCode> {
    let mut conn = pool.lock().unwrap();
    let product = create_product(&mut conn, &payload.product_name, &payload.product_value);
    Ok((StatusCode::CREATED, Json(product)))
}

fn create_product_and_tax(conn: &mut SqliteConnection) {
    let name = "phone";
    let value = 100;

    let product = create_product(conn, &name, &value);

    let state = "California";
    let year_var = 100;
    let percent_var = 100;
    let product_id_var = product.id;
    // the imports on lines 43, 44 imports the struct variables and I can not use them, just giving different names whiles I do not find a better solution

    let _ = create_tax(conn, &state, &year_var, &percent_var, &product_id_var);
}
#[tokio::main]
async fn main() {
    let connection = establish_connection();
    let pool: DbPool = Arc::new(Mutex::new(connection));

    let app = Router::new()
        .route("/products/{id}", get(get_product_handler))
        .route("/products", post(create_product_handler))
        .with_state(pool);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Server running on http://localhost:3000");
    axum::serve(listener, app).await.unwrap();
}
