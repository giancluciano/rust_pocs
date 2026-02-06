use sqlx::sqlite::SqlitePoolOptions;

#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    let pool = SqlitePoolOptions::new().connect("database.db").await?;
    sqlx::migrate!().run(&pool).await?;
    println!("connected to SQLite database and migrations applied");
    Ok(())
}
