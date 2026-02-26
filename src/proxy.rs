use axum::{
    body::{self, Body, Bytes},
    extract::{Path, Request, State},
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
};
use hyper::Method;

use crate::state::AppState;

pub async fn proxy_handler(
    Path(path): Path<String>,
    State(state): State<AppState>,
    req: Request<Body>,
) -> impl IntoResponse {
    let headers = req.headers().clone();
    let method: Method = req.method().clone();
    // consume body with an arbitrary max size
    let body = req.into_body();
    let body_bytes = match body::to_bytes(body, 8 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_REQUEST, "Failed to read body").into_response(),
    };

    // simple bearer key check
    if let Some(auth) = headers.get("authorization") {
        if let Ok(auth_str) = auth.to_str() {
            if let Some(key) = auth_str.strip_prefix("Bearer ") {
                if state.valid_keys.contains(&key.to_string()) {
                    return forward_request(&state, method, path, headers, body_bytes).await;
                }
            }
        }
    }

    (StatusCode::UNAUTHORIZED, "Unauthorized").into_response()
}

pub async fn forward_request(
    state: &AppState,
    method: Method,
    path: String,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let base = state.ollama_url.trim_end_matches('/');
    let url = format!("{}/v1/{}", base, path);

    // reqwest expects its own Method type; convert from hyper's.
    let reqwest_method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);
    let mut req = state.client.request(reqwest_method, &url).body(body.clone());

    for (name, value) in headers.iter() {
        if name == "host" || name == "authorization" {
            continue;
        }
        if let Ok(val_str) = value.to_str() {
            req = req.header(name.as_str(), val_str);
        }
    }

    match req.send().await {
        Ok(resp) => {
            let status_code =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::OK);
            let mut response_builder = Response::builder().status(status_code);
            for (name, value) in resp.headers().iter() {
                if let Ok(val_str) = value.to_str() {
                    response_builder = response_builder.header(name.as_str(), val_str);
                }
            }
            let bytes = resp.bytes().await.unwrap_or_default();
            response_builder
                .body(Body::from(bytes))
                .unwrap_or_else(|_| {
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::empty())
                        .unwrap()
                })
        }
        Err(err) => {
            eprintln!("error forwarding request: {err}");
            Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from("Upstream request failed"))
                .unwrap()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use axum::body::Body;
    use axum::http::Request;
    use axum::http::StatusCode;
    use httpmock::MockServer;
    use reqwest::Client;

    #[tokio::test]
    async fn unauthorized_missing_header() {
        let state = AppState {
            client: Client::new(),
            valid_keys: vec!["secret".into()],
            ollama_url: "http://localhost".into(),
        };
        let req = Request::builder().body(Body::from("")).unwrap();
        let resp = proxy_handler(Path("foo".into()), State(state), req)
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn forward_happy_path() {
        let server = MockServer::start_async().await;
        let mock = server.mock(|when, then| {
            when.method("POST").path("/v1/test");
            then.status(200).body("ok");
        });

        let state = AppState {
            client: Client::new(),
            valid_keys: vec!["goodkey".into()],
            ollama_url: server.url(""),
        };

        let req = Request::builder()
            .method(Method::POST)
            .header("authorization", "Bearer goodkey")
            .body(Body::from("hello"))
            .unwrap();

        let resp = proxy_handler(Path("test".into()), State(state), req)
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        mock.assert();
    }

    #[tokio::test]
    async fn forward_get_method() {
        let server = MockServer::start_async().await;
        let mock = server.mock(|when, then| {
            when.method("GET").path("/v1/test-get");
            then.status(200).body("okget");
        });

        let state = AppState {
            client: Client::new(),
            valid_keys: vec!["goodkey".into()],
            ollama_url: server.url(""),
        };

        let req = Request::builder()
            .method(Method::GET)
            .header("authorization", "Bearer goodkey")
            .body(Body::from(""))
            .unwrap();

        let resp = proxy_handler(Path("test-get".into()), State(state), req)
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        mock.assert();
    }
}
