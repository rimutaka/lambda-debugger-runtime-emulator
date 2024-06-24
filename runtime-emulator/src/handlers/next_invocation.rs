use super::full;
use crate::sqs;
use http_body_util::combinators::BoxBody;
use hyper::body::Bytes;
use hyper::Error;
use hyper::Response;
use tracing::info;

/// Handles _next invocation_ request from the local lambda.
/// It blocks on SQS and waits indefinitely for the next SQS message to arrive.
/// The first message in the queue is passed back onto the local lambda.
/// See https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html#runtimes-api-next
pub(crate) async fn handler() -> Response<BoxBody<Bytes, Error>> {
    // get the next SQS message or wait for it to arrive
    // this call will block until a message is available
    let sqs_message = sqs::get_input().await;

    info!("Lambda request:\n{}", sqs_message.payload);

    Response::builder()
        .status(hyper::StatusCode::OK)
        .header("lambda-runtime-aws-request-id", sqs_message.receipt_handle)
        .header("lambda-runtime-deadline-ms", sqs_message.ctx.deadline)
        .header(
            "lambda-runtime-invoked-function-arn",
            sqs_message.ctx.invoked_function_arn,
        )
        .header(
            "lambda-runtime-trace-id",
            sqs_message.ctx.xray_trace_id.unwrap_or_else(|| {
                "Root=0-00000000-000000000000000000000000;Parent=0000000000000000;Sampled=0;Lineage=00000000:0"
                    .to_owned()
            }),
        )
        .body(full(sqs_message.payload))
        .expect("Failed to create a response")
}
