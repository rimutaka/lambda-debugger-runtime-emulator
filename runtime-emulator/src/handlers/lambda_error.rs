use super::{empty, BLOCK_NEXT_INVOCATION};
use http_body_util::{combinators::BoxBody, BodyExt};
use hyper::body::Bytes;
use hyper::Error;
use hyper::{Request, Response};
use tracing::{debug, error, info};

pub(crate) async fn handler(req: Request<hyper::body::Incoming>) -> Response<BoxBody<Bytes, Error>> {
    // Initialization error (https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html#runtimes-api-initerror) and
    // Invocation error (https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html#runtimes-api-invokeerror)
    // are rolled together into a single handler because it is not clear how to handle errors
    // and if the error should be propagated upstream
    let resp = match req.into_body().collect().await {
        Ok(v) => v.to_bytes(),
        Err(e) => panic!("Failed to read lambda response: {:?}", e),
    };

    match String::from_utf8(resp.as_ref().to_vec()) {
        Ok(v) => {
            info!("Lambda error: {v}");
        }
        Err(e) => {
            error!(
                "Non-UTF-8 error response from Lambda. {:?}\n{}",
                e,
                hex::encode(resp.as_ref())
            );
        }
    }

    // block the next invocation to prevent an infinite loop of reruns
    if let Ok(mut w) = BLOCK_NEXT_INVOCATION.write() {
        debug!("Blocking the next invocation");
        *w = true;
    } else {
        error!("Write deadlock on BLOCK_NEXT_INVOCATION. It's a bug");
    }

    // lambda allows for more informative error responses, but this may be enough for now
    Response::builder()
        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
        .body(empty())
        .expect("Failed to create a response")
}
