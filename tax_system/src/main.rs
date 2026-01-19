use diesel::prelude::*;
use dotenvy::dotenv;
use std::env;

pub mod models;
pub mod schema;

use self::models::*;

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
        .expect("Error saving new post")
}

fn main() {
    use self::schema::products::dsl::*;
    
    let connection = &mut establish_connection();


    let name = "phone";
    let value = 100;

    let _ = create_product(connection, &name, &value);

    let results = products
        .limit(5)
        .select(Product::as_select())
        .load(connection)
        .expect("Error loading products");

    println!("Displaying {} products", results.len());
    for product in results {
        println!("{}", product.product_name);
        println!("-----------\n");
    }
}
