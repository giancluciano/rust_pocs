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
    use axum::{body::Body, http::{Request, StatusCode, Method}};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn build_app() -> Router {
        Router::new().route("/", get(hello_world))
    }

    #[tokio::test]
    async fn test_hello_world_status_and_body() {
        let response = build_app()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"Hello, World!");
    }

    #[tokio::test]
    async fn test_hello_world_content_type_is_text() {
        let response = build_app()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        assert!(content_type.contains("text/plain"), "expected text/plain, got: {content_type}");
    }

    #[tokio::test]
    async fn test_unknown_route_returns_404() {
        let response = build_app()
            .oneshot(Request::builder().uri("/unknown").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_post_to_root_returns_405() {
        let response = build_app()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}
