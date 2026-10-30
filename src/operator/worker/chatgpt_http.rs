//! SIWC's public Responses route can stream SSE without a Content-Type header.
use rig::http_client::{
    Bytes, DynHttpClient, HttpClientExt, LazyBody, MultipartForm, Request, Response,
    StreamingResponse,
};
use std::future::Future;

struct ChatGptStreamHttp {
    inner: rig::rig_reqwest::ReqwestClient,
    responses_uri: String,
}
pub(super) fn client(base: &str) -> DynHttpClient {
    DynHttpClient::new(ChatGptStreamHttp {
        inner: rig::rig_reqwest::ReqwestClient::default(),
        responses_uri: format!("{}/responses", base.trim_end_matches('/')),
    })
}
impl HttpClientExt for ChatGptStreamHttp {
    fn send<T, U>(
        &self,
        request: Request<T>,
    ) -> impl Future<Output = rig::http_client::Result<Response<LazyBody<U>>>> + Send + 'static
    where
        T: Into<Bytes> + Send,
        U: From<Bytes> + Send + 'static,
    {
        self.inner.send(request)
    }
    fn send_multipart<U>(
        &self,
        request: Request<MultipartForm>,
    ) -> impl Future<Output = rig::http_client::Result<Response<LazyBody<U>>>> + Send + 'static
    where
        U: From<Bytes> + Send + 'static,
    {
        self.inner.send_multipart(request)
    }
    fn send_streaming<T>(
        &self,
        request: Request<T>,
    ) -> impl Future<Output = rig::http_client::Result<StreamingResponse>> + Send
    where
        T: Into<Bytes> + Send,
    {
        let request: Request<Bytes> = request.map(Into::into);
        let subscription_stream = request.method() == rig::http_client::Method::POST
            && request.uri().to_string() == self.responses_uri
            && serde_json::from_slice::<serde_json::Value>(request.body())
                .ok()
                .and_then(|body| body.get("stream").and_then(serde_json::Value::as_bool))
                == Some(true);
        let response = self.inner.send_streaming(request);
        async move {
            let mut response = response.await?;
            if subscription_stream
                && response.status() == rig::http_client::StatusCode::OK
                && !response.headers().contains_key("content-type")
            {
                response.headers_mut().insert(
                    "content-type",
                    rig::http_client::HeaderValue::from_static("text/event-stream"),
                );
            }
            Ok(response)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn headerless_route_gate_preserves_method_uri_stream_status_and_explicit_type() {
        for (method, path, stream, status, explicit_type, normalized) in [
            ("POST", "/responses", true, 200, false, true),
            ("GET", "/responses", true, 200, false, false),
            ("POST", "/other", true, 200, false, false),
            ("POST", "/responses?other=1", true, 200, false, false),
            ("POST", "/responses", false, 200, false, false),
            ("POST", "/responses", true, 201, false, false),
            ("POST", "/responses", true, 200, true, false),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    let mut block = [0; 1024];
                    let count = socket.read(&mut block).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&block[..count]);
                }
                let content_type = if explicit_type {
                    "Content-Type: application/json\r\n"
                } else {
                    ""
                };
                socket.write_all(format!("HTTP/1.1 {status} Fixture\r\n{content_type}Content-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            });
            let http = client(&base);
            let request = Request::builder()
                .method(method)
                .uri(format!("{base}{path}"))
                .body(serde_json::json!({"stream":stream}).to_string())
                .unwrap();
            let response = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                http.send_streaming(request),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(response.status().as_u16(), status);
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok());
            assert_eq!(
                content_type,
                if normalized {
                    Some("text/event-stream")
                } else if explicit_type {
                    Some("application/json")
                } else {
                    None
                }
            );
            server.await.unwrap();
        }
    }
}
