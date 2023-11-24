use std::collections::HashMap;
use std::convert::Into;
use std::str::FromStr;

use http_body_util::Empty;
use hyper::body::{Body, Bytes};
use hyper::header::{HeaderName, IntoHeaderName};
use hyper::http::uri::PathAndQuery;
use hyper::http::HeaderValue;
use hyper::{Method, Request, Response, Uri};
use hyper_util::client::legacy::connect::Connect;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

pub fn build_request(
    base: Uri,
    path_and_query: &str,
    method: Method,
) -> Result<Request<Empty<Bytes>>, color_eyre::Report> {
    build_request_with_headers_and_body::<_, HeaderName>(
        base,
        path_and_query,
        HashMap::default(),
        method,
        Empty::<Bytes>::new(),
    )
}

#[allow(unused)]
pub fn build_request_with_body<B>(
    base: Uri,
    path_and_query: &str,
    method: Method,
    body: B,
) -> Result<Request<B>, color_eyre::Report>
where
    B: Body + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    build_request_with_headers_and_body::<B, HeaderName>(
        base,
        path_and_query,
        HashMap::default(),
        method,
        body,
    )
}

#[allow(unused)]
pub fn build_request_with_headers<K>(
    base: Uri,
    path_and_query: &str,
    headers: HashMap<K, HeaderValue>,
    method: Method,
) -> Result<Request<Empty<Bytes>>, color_eyre::Report>
where
    K: IntoHeaderName,
{
    build_request_with_headers_and_body(
        base,
        path_and_query,
        headers,
        method,
        Empty::<Bytes>::new(),
    )
}

pub fn build_request_with_headers_and_body<B, K>(
    base: Uri,
    path_and_query: &str,
    headers: HashMap<K, HeaderValue>,
    method: Method,
    body: B,
) -> Result<Request<B>, color_eyre::Report>
where
    B: Body + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    K: IntoHeaderName,
{
    let full_url = build_uri(base, path_and_query)?;

    let mut request = Request::builder()
        .uri(full_url)
        .method(method)
        .body::<B>(body)?;

    let request_headers = request.headers_mut();

    for (k, v) in headers {
        request_headers.insert(k, v);
    }

    Ok(request)
}

pub async fn execute_request<C, B>(
    connector: C,
    request: Request<B>,
) -> Result<Response<hyper::body::Incoming>, color_eyre::Report>
where
    C: Connect + Clone + Send + Sync + 'static,
    B: Body + Send + Unpin + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let client = Client::builder(TokioExecutor::new()).build::<_, B>(connector);

    let response = client.request(request).await?;

    Ok(response)
}

pub fn build_uri(base_url: Uri, path_and_query: &str) -> Result<Uri, color_eyre::Report> {
    let mut parts = base_url.into_parts();

    parts.path_and_query = Some(PathAndQuery::from_str(path_and_query)?);

    Uri::from_parts(parts).map_err(Into::into)
}
