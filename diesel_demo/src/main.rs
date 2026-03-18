pub mod schema;
pub mod models;

use self::models::*;
use diesel::prelude::*;
use dotenvy::dotenv;
use std::env;

#[cfg(test)]
mod tests {
    use super::*;
    use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};

    const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

    fn setup_test_db() -> SqliteConnection {
        let mut conn = SqliteConnection::establish(":memory:")
            .expect("Failed to create in-memory SQLite database");
        conn.run_pending_migrations(MIGRATIONS)
            .expect("Failed to run migrations");
        conn
    }

    #[test]
    fn test_create_post_returns_post_with_correct_fields() {
        let conn = &mut setup_test_db();
        let post = create_post(conn, "Test Title", "Test Body");
        assert_eq!(post.title, "Test Title");
        assert_eq!(post.body, "Test Body");
        assert!(!post.published);
    }

    #[test]
    fn test_create_post_assigns_id() {
        let conn = &mut setup_test_db();
        let post = create_post(conn, "Another Post", "Some body");
        assert!(post.id > 0);
    }

    #[test]
    fn test_create_multiple_posts_have_unique_ids() {
        let conn = &mut setup_test_db();
        let post1 = create_post(conn, "Post 1", "Body 1");
        let post2 = create_post(conn, "Post 2", "Body 2");
        assert_ne!(post1.id, post2.id);
    }

    #[test]
    fn test_created_post_is_persisted_in_db() {
        use crate::schema::posts::dsl::*;

        let conn = &mut setup_test_db();
        create_post(conn, "Persisted Post", "Persisted Body");

        let results = posts
            .select(Post::as_select())
            .load(conn)
            .expect("Error loading posts");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Persisted Post");
    }

    #[test]
    fn test_filter_by_published_false() {
        use crate::schema::posts::dsl::*;

        let conn = &mut setup_test_db();
        create_post(conn, "Unpublished", "body");

        let results = posts
            .filter(published.eq(false))
            .select(Post::as_select())
            .load(conn)
            .expect("Error loading posts");

        assert_eq!(results.len(), 1);
        assert!(!results[0].published);
    }
}

pub fn establish_connection() -> SqliteConnection {
    dotenv().ok();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    SqliteConnection::establish(&database_url)
        .unwrap_or_else(|_| panic!("Error connecting to {}", database_url))
}

pub fn create_post(conn: &mut SqliteConnection, title: &str, body: &str) -> Post {
    use crate::schema::posts;

    let new_post = NewPost { title, body };

    diesel::insert_into(posts::table)
        .values(&new_post)
        .returning(Post::as_returning())
        .get_result(conn)
        .expect("Error saving new post")
}

fn main() {
    use self::schema::posts::dsl::*;

    let connection = &mut establish_connection();

    let mut title_name = "New post";
    let mut body_name = "post body";
    create_post(connection, &title_name, &body_name);

    let results = posts
        .filter(published.eq(false))
        .limit(5)
        .select(Post::as_select())
        .load(connection)
        .expect("Error loading posts");

    println!("Displaying {} posts", results.len());
    for post in results {
        println!("{}", post.title);
        println!("-----------\n");
        println!("{}", post.body);
    }
}
