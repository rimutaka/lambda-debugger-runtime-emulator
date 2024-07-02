use super::empty;
use http_body_util::{combinators::BoxBody, BodyExt};
use hyper::body::Bytes;
use hyper::Error;
use hyper::{Request, Response};
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

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
            warn!(
                "Non-UTF-8 error response from Lambda. {:?}\n{}",
                e,
                hex::encode(resp.as_ref())
            );
        }
    }

    info!("Ctlr-C your lambda within 30s to avoid a rerun");
    sleep(Duration::from_secs(30)).await;

    // lambda allows for more informative error responses, but this may be enough for now
    Response::builder()
        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
        .body(empty())
        .expect("Failed to create a response")
}
