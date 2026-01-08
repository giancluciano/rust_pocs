use axum::{response::{Response, IntoResponse}, Json, http::StatusCode};
use serde_json::json;
use serde::Serialize;
use axum::{Router, routing::get};
use axum::Json;
use serde_json::Value;

// here we show a type that implements Serialize + Send
#[derive(Serialize)]
struct Message {
    message: String
}

enum ApiResponse {
    OK,
    Created,
    JsonData(Vec<Message>),
}

enum ApiError {
    BadRequest,
    Forbidden,
    Unauthorised,
    InternalServerError
}

impl IntoResponse for ApiResponse {
    fn into_response(self) -> Response {
        match self {
            Self::OK => (StatusCode::OK).into_response(),
            Self::Created => (StatusCode::CREATED).into_response(),
            Self::JsonData(data) => (StatusCode::OK, Json(data)).into_response()
        }
    }
}

async fn get_user() -> Result<ApiResponse, ApiError> {
    // Return a dummy user for poc
    let data = vec![
        Message { message: "Dummy User".to_string() }
    ];
    Ok(ApiResponse::JsonData(data))
}

#[tokio::main]
async fn main() {
    // build our application with a single route
    let app = Router::new()
        .route("/user", get(get_user));

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}