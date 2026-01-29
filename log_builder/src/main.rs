use axum::{routing::get, Router};
use elasticsearch::{
    Elasticsearch, IndexParts, cat::CatIndicesParts, http::transport::Transport
};
use serde_json::{Value, json};

async fn hello_world() -> &'static str {
    "Hello, World!"
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let transport = Transport::single_node("http://localhost:9200")?;
    let client = Elasticsearch::new(transport);

    let response = client
        .index(IndexParts::IndexId("system", "1"))
        .body(json!({
            "message": "system started"
        }))
        .send()
        .await?;

    let response = client
        .cat()
        .indices(CatIndicesParts::Index(&["*"]))
        .format("json")
        .send()
        .await?;

    let response_body = response.json::<Value>().await?;
    for record in response_body.as_array().unwrap() {
        // print the name of each index
        println!("{}", record["index"].as_str().unwrap());
    }
    let app = Router::new().route("/", get(hello_world));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Server running on http://localhost:3000");
    axum::serve(listener, app).await.unwrap();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_hello_world_endpoint() {
        let app = Router::new().route("/", get(hello_world));

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), 200);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"Hello, World!");
    }
}
