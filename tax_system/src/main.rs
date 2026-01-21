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
fn main() {
    use self::schema::products::dsl::*;
    use self::schema::taxes::dsl::*;
    
    let connection = &mut establish_connection();

    // create_product_and_tax(connection);
    let results = products
        .limit(5)
        .select(Product::as_select())
        .load(connection)
        .expect("Error loading products");

    let tax_results = taxes
        .limit(5)
        .select(Tax::as_select())
        .load(connection)
        .expect("Error loading taxes");

    println!("Displaying {} products", results.len());
    for product in results {
        println!("{}", product.product_name);
        println!("-----------\n");
    }

    println!("Displaying {} taxes", tax_results.len());
    for tax in tax_results {
        println!("{} {} {}", tax.state_name, tax.year, tax.percent);
        println!("-----------\n");
    }
}
