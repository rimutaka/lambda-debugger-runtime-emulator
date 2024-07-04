use super::{full, BLOCK_NEXT_INVOCATION, LOCAL_REQUEST_ID};
use crate::config::PayloadSources;
use crate::sqs;
use crate::CONFIG;
use http_body_util::combinators::BoxBody;
use hyper::body::Bytes;
use hyper::Error;
use hyper::Response;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

/// Handles _next invocation_ request from the local lambda.
/// It blocks on SQS and waits indefinitely for the next SQS message to arrive.
/// The first message in the queue is passed back onto the local lambda.
/// See https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html#runtimes-api-next
pub(crate) async fn handler() -> Response<BoxBody<Bytes, Error>> {
    // check if the current invocation is a re-run and should be blocked
    block_if_rerun().await;

    // check if there is a payload file name in the command line arguments
    let config = CONFIG.get().await;

    // return local payload from the file if was provided
    if let PayloadSources::Local(local_config) = &config.sources {
        info!("Lambda request: sending payload from file");

        return Response::builder()
            .status(hyper::StatusCode::OK)
            .header("lambda-runtime-aws-request-id", LOCAL_REQUEST_ID)
            .header("lambda-runtime-deadline-ms", "2035313041000") // 2034
            .header("lambda-runtime-invoked-function-arn", "from-local-payload")
            .header(
                "lambda-runtime-trace-id",
                "Root=0-00000000-000000000000000000000000;Parent=0000000000000000;Sampled=0;Lineage=00000000:0",
            )
            .body(full(local_config.payload.clone()))
            .expect("Failed to create a response");
    };

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

/// Checks BLOCK_NEXT_INVOCATION global flag and
/// blocks the current thread if the current invocation should be blocked.
async fn block_if_rerun() {
    // create a local copy of the blocking flag
    let block = if let Ok(block) = BLOCK_NEXT_INVOCATION.read() {
        *block
    } else {
        error!("Read deadlock on BLOCK_NEXT_INVOCATION. It's a bug");
        false
    };

    // unblock the next invocation
    if block {
        if let Ok(mut w) = BLOCK_NEXT_INVOCATION.write() {
            *w = false;
        } else {
            error!("Write deadlock on BLOCK_NEXT_INVOCATION. It's a bug");
        }
    }

    // sleep for a month to prevent a rerun
    if block {
        warn!("Restart your lambda for a rerun");
        sleep(Duration::from_secs(31563000)).await;
    }
}
