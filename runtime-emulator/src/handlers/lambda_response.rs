use super::empty;
use crate::sqs;
use http_body_util::{combinators::BoxBody, BodyExt};
use hyper::body::Bytes;
use hyper::Error;
use hyper::Request;
use hyper::Response;
use regex::Regex;
use std::sync::OnceLock;
use tracing::info;

/// Contains compiled regex for extracting the receipt handle from the URL.
static RECEIPT_REGEX: OnceLock<Regex> = OnceLock::new();

/// Handles an invocation response the local lambda when it successfully completed processing.
/// We forward the response to the SQS queue where it is picked up by the remote proxy lambda
/// that forwards it to the original caller, e.g. API Gateway.
/// See https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html#runtimes-api-response
///
/// Lambda invocations are async in nature - the lambda picks up an invocation as a response from the runtime,
/// does the processing and then sends another request to the runtime with the invocation/request ID in the URL.
pub(crate) async fn handler(req: Request<hyper::body::Incoming>) -> Response<BoxBody<Bytes, Error>> {
    // The regex extracts the receipt handle from the path, e.g. /runtime/invocation/[aws-req-id]/response
    // where the request ID in the URL is the receipt handle for SQS - it is not the actual lambda request ID.
    // We need to store the receipt handle somewhere and placing it into the request-id param seems like an easy way to do it
    // because the local lambda will return it with the response.
    // The receipt handle can be a long string with /, - and other non-alphanumeric characters.

    let regex = RECEIPT_REGEX.get_or_init(|| {
        Regex::new(r"/runtime/invocation/(.+)/response").expect("Invalid response URL regex. It's a bug.")
    });
    let receipt_handle = regex
        .captures(req.uri().path())
        .unwrap_or_else(|| panic!("URL parsing regex failed on: {:?}. It' a bug", req.uri()))
        .get(1)
        .unwrap_or_else(|| {
            panic!(
                "Request URL does not conform to /runtime/invocation/AwsRequestId/response: {:?}",
                req.uri()
            )
        })
        .as_str()
        .to_owned();

    // convert the lambda response to bytes
    let response = match req.into_body().collect().await {
        Ok(v) => v.to_bytes(),
        Err(e) => panic!("Failed to read lambda response: {:?}", e),
    };

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

    info!("Lambda response:\n{sqs_payload}");

    sqs::send_output(sqs_payload, receipt_handle).await;

    Response::builder()
        .status(hyper::StatusCode::OK)
        .body(empty())
        .expect("Failed to create a response")
}
