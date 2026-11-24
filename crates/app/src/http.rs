use std::sync::Arc;

use futures::future::BoxFuture;
use gpui_kit::http_client::{AsyncBody, HttpClient, Url};
use http::{Request, Response, header::HeaderValue};
use tokio::runtime::{Handle, Runtime};
use wreq_util::Emulation;

pub struct WreqHttpClient {
    runtime: Handle,
    _runtime_owner: Arc<Runtime>,
    client: wreq::Client,
}

impl WreqHttpClient {
    pub fn new() -> Self {
        let runtime = Runtime::new().expect("http runtime");
        let handle = runtime.handle().clone();
        let client = handle.block_on(async {
            wreq::Client::builder()
                .emulation(Emulation::Chrome149)
                .build()
                .expect("http client")
        });
        Self {
            runtime: handle,
            _runtime_owner: Arc::new(runtime),
            client,
        }
    }
}

impl Default for WreqHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient for WreqHttpClient {
    fn user_agent(&self) -> Option<&HeaderValue> {
        None
    }

    fn proxy(&self) -> Option<&Url> {
        None
    }

    fn send(
        &self,
        req: Request<AsyncBody>,
    ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
        let client = self.client.clone();
        let runtime = self.runtime.clone();
        Box::pin(async move {
            let (parts, _body) = req.into_parts();
            runtime
                .spawn(async move {
                    let method = wreq::Method::from_bytes(parts.method.as_str().as_bytes())?;

                    let mut request = client.request(method, parts.uri.to_string());
                    for (name, value) in parts.headers.iter() {
                        request = request.header(name, value);
                    }

                    let response = request.send().await?;
                    let status = response.status();
                    let headers = response.headers().clone();
                    let bytes = response.bytes().await?;

                    let mut builder = Response::builder().status(status);
                    for (name, value) in headers.iter() {
                        builder = builder.header(name, value);
                    }
                    anyhow::Ok(builder.body(AsyncBody::from_bytes(bytes))?)
                })
                .await?
        })
    }
}
