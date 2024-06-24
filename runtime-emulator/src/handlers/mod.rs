use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::body::Bytes;

pub(crate) mod lambda_error;
pub(crate) mod lambda_response;
pub(crate) mod next_invocation;

/// Returns an empty response body.
pub(crate) fn empty() -> BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new().map_err(|never| match never {}).boxed()
}

/// Returns an response body with contents of `chunk` which can be some type convertible into Bytes, e.g. &str.
pub(crate) fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(chunk.into()).map_err(|never| match never {}).boxed()
}
