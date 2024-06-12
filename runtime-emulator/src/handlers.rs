use crate::sqs;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::body::Bytes;
use hyper::Error;
use hyper::Response;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

pub(crate) async fn api_next_invocation() -> Response<BoxBody<Bytes, Error>> {
    // get the next SQS message or wait for it to arrive
    // this call will block until a message is available
    let sqs_message = sqs::get_input().await;

    info!("{}", sqs_message.payload);

    Response::builder()
        .status(hyper::StatusCode::OK)
        .header("lambda-runtime-aws-request-id", sqs_message.receipt_handle)
        .header("lambda-runtime-deadline-ms", sqs_message.ctx.deadline)
        .header(
            "lambda-runtime-invoked-function-arn",
            sqs_message.ctx.invoked_function_arn,
        )
        .header("lambda-runtime-trace-id", sqs_message.ctx.xray_trace_id)
        .body(full(sqs_message.payload))
        .expect("Failed to create a response")
}

pub(crate) async fn api_response(response: Bytes, receipt_handle: String) -> Response<BoxBody<Bytes, Error>> {
    let sqs_payload = match String::from_utf8(response.as_ref().to_vec()) {
        Ok(v) => v,
        Err(e) => {
            panic!(
                "Non-UTF-8 response from Lambda. {:?}\n{}",
                e,
                hex::encode(response.as_ref())
            );
        }
    };

    info!("Lambda response: {sqs_payload}");

    sqs::send_output(sqs_payload, receipt_handle).await;

    Response::builder()
        .status(hyper::StatusCode::OK)
        .body(empty())
        .expect("Failed to create a response")
}

pub(crate) async fn api_error(resp: Bytes) -> Response<BoxBody<Bytes, Error>> {
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

    info!("Ctlr-C your lambda or this runtime within 10s to avoid a rerun");
    sleep(Duration::from_secs(10)).await;

    // lambda allows for more informative error responses, but this may be enough for now
    Response::builder()
        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
        .body(empty())
        .expect("Failed to create a response")
}

// We create some utility functions to make Empty and Full bodies
// fit our broadened Response body type.
fn empty() -> BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new().map_err(|never| match never {}).boxed()
}

fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(chunk.into()).map_err(|never| match never {}).boxed()
}
