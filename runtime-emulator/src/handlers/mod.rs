use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::body::Bytes;
use std::sync::RwLock;

pub(crate) mod lambda_error;
pub(crate) mod lambda_response;
pub(crate) mod next_invocation;

/// A request ID substitute for local file payloads.
/// No SQS responses are sent back to AWS for this request ID.
pub(crate) const LOCAL_REQUEST_ID: &str = "local-request-id";

/// Is set to TRUE if the next invocation will be using the same payload resulting
/// in an infinite loop. It happens with SUCCESS responses for local payloads and all ERROR responses.
/// It is set while processing the response (success or error).
/// Once an invocation is blocked, it is reset to FALSE to let the next invocation can go ahead. 
pub(crate) static BLOCK_NEXT_INVOCATION: RwLock<bool> = RwLock::new(false);

/// Returns an empty response body.
pub(crate) fn empty() -> BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new().map_err(|never| match never {}).boxed()
}

/// Returns an response body with contents of `chunk` which can be some type convertible into Bytes, e.g. &str.
pub(crate) fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(chunk.into()).map_err(|never| match never {}).boxed()
}
