use crate::CONFIG;
use async_once::AsyncOnce;
use aws_sdk_sqs::{types::Message, Client as SqsClient};
use flate2::read::GzEncoder;
use flate2::Compression;
use lambda_runtime::Context as Ctx;
use lazy_static::lazy_static;
use runtime_emulator_types::RequestPayload;
use std::io::prelude::*;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

// Cannot use OnceCell because it does not support async initialization
lazy_static! {
    pub(crate) static ref SQS_CLIENT: AsyncOnce<SqsClient> =
        AsyncOnce::new(async { SqsClient::new(&aws_config::load_from_env().await) });
}

/// A parsed SQS message.
/// The parsing is limited to extracting the data we need and passing the rest to the runtime.
#[derive(Debug)]
pub(crate) struct SqsMessage {
    pub payload: String,
    /// the message receipt is needed to delete the message from the queue later
    pub receipt_handle: String,
    /// From the context
    pub ctx: Ctx,
}

/// Reads a message from the specified SQS queue and returns the payload as Lambda structures
pub(crate) async fn get_input() -> SqsMessage {
    let config = CONFIG.get().await;
    let client = SQS_CLIENT.get().await;

    // time to wait for the next message in seconds
    // set to 0 to begin with a friendly message logic
    let mut wait_time = 0;

    // start listening to the response
    loop {
        // try to get the next message and wait for it to arrive if none is ready
        // sleep for a bit on error before retrying
        let resp = match client
            .receive_message()
            .max_number_of_messages(1)
            .set_queue_url(Some(config.request_queue_url.clone()))
            .set_wait_time_seconds(Some(wait_time))
            .send()
            .await
        {
            Ok(v) => v,
            Err(e) => {
                warn!("Failed to get messages: {}", e);
                sleep(Duration::from_millis(5000)).await;
                continue;
            }
        };

        // wait until a message arrives or the function is killed by AWS
        if resp.messages.is_none() {
            // print a friendly reminder to send an event
            if wait_time == 0 {
                info!("Lambda connected. Waiting for an incoming event from AWS.");
                wait_time = 20;
            }

            continue;
        }

        // SQS returns an empty list returns when the queue wait time expires
        let mut msgs = resp.messages.expect("Failed to get list of messages");

        // extract the payload and the receipt handle
        let (payload, receipt_handle) = if let Some(msg) = msgs.pop() {
            match msg {
                Message {
                    body: Some(body),
                    receipt_handle: Some(receipt_handle),
                    ..
                } => (body, receipt_handle),
                _ => panic!("Invalid SQS message. Missing body or receipt: {:?}", msg),
            }
        } else {
            // no messages in the queue
            continue;
        };

        // the SQS payload contains event and context that need to be extracted
        // there is no way to pass the context to the lambda, but we can at least log it
        // the payload that is passed to the lambda is in event property

        // {
        //     "event": { "command": "value1", "key2": "value2", "key3": "value3" },
        //     "ctx":
        //       {
        //         "request_id": "4850539c-6316-4af1-9c47-8771cb3baeb1",
        //         "deadline": 1718071341165,
        //         "invoked_function_arn": "arn:aws:lambda:us-east-1:512295225992:function:lambda-debug-proxy",
        //         "xray_trace_id": "Root=1-6667af77-3f5a28b931d7678525d90593;Parent=66ab8e86299a69bc;Sampled=0;Lineage=8af230b3:0",
        //         "client_context": null,
        //         "identity": null,
        //         "env_config":
        //           {
        //             "function_name": "lambda-debug-proxy",
        //             "memory": 128,
        //             "version": "$LATEST",
        //             "log_stream": "2024/06/11/lambda-debug-proxy[$LATEST]b1de3d3cab074896b448859c52fa1a2d",
        //             "log_group": "/aws/lambda/lambda-debug-proxy",
        //           },
        //       },
        //   }

        let payload: RequestPayload = serde_json::from_str(&payload).expect("Failed to deserialize msg body");
        let ctx = payload.ctx;

        let payload = serde_json::to_string(&payload.event).expect("event contents cannot be serialized");

        // if we reached this point, we have a parsed SQS message
        // with the payload and the receipt handle
        // and should return it to the caller
        return SqsMessage {
            payload,
            receipt_handle,
            ctx,
        };
    }
}

/// Returns URLs of the default request and response queues, if they exist.
pub(crate) async fn get_default_queues() -> (Option<String>, Option<String>) {
    let client = SQS_CLIENT.get().await;

    // example of the default request queue URL
    // https://sqs.us-east-1.amazonaws.com/512295225992/proxy_lambda_req

    // get the list of queues that start with the default queue prefix
    let resp = match client
        .list_queues()
        .set_queue_name_prefix(Some("proxy_lambda_re".to_string()))
        .set_max_results(Some(100))
        .send()
        .await
    {
        Ok(v) => v,
        Err(e) => {
            panic!("Failed to get list of SQS queues: {}", e);
        }
    };

    // output containers
    let mut req_queue = None;
    let mut resp_queue = None;

    // match queue names against the default names
    if let Some(queue_urls) = resp.queue_urls {
        for url in queue_urls {
            if url.ends_with("/proxy_lambda_req") {
                req_queue = Some(url);
            } else if url.ends_with("/proxy_lambda_resp") {
                resp_queue = Some(url);
            }
        }
    }

    (req_queue, resp_queue)
}

/// Send back the response and delete the message from the queue.
pub(crate) async fn send_output(response: String, receipt_handle: String) {
    let config = CONFIG.get().await;
    let client = SQS_CLIENT.get().await;

    let response_queue_url = match &config.response_queue_url {
        Some(v) => v.clone(),
        None => {
            info!("Response dropped: no response queue configured");
            return;
        }
    };

    let response = compress_output(response);

    // SQS messages must be shorter than 262144 bytes
    if response.len() < 262144 {
        if let Err(e) = client
            .send_message()
            .set_message_body(Some(response))
            .set_queue_url(Some(response_queue_url))
            .send()
            .await
        {
            panic!("Failed to send SQS response: {}", e);
        };
    } else {
        info!(
            " Response dropped: message size {}B, max allowed by SQS is 262,144 bytes",
            response.len()
        );
    }

    // delete the request msg from the queue so it cannot be replayed again
    if let Err(e) = client
        .delete_message()
        .set_queue_url(Some(config.request_queue_url.to_string()))
        .set_receipt_handle(Some(receipt_handle))
        .send()
        .await
    {
        panic!("Failed to send SQS response: {}", e);
    };

    info!("Response sent and request deleted from the queue");
}

/// Compresses and encodes the output as Base58 if the message is larger than what is
/// allowed in SQS (262,144 bytes)
fn compress_output(response: String) -> String {
    // is it small enough to fit in?
    if response.len() < 262144 {
        return response;
    }

    info!(
        "Message size: {}B, max allowed: 262144B. Compressing...",
        response.len()
    );

    // try to decompress the body
    let mut gzipper = GzEncoder::new(response.as_bytes(), Compression::fast());
    let mut gzipped: Vec<u8> = Vec::new();
    let compressed_len = match gzipper.read_to_end(&mut gzipped) {
        Ok(v) => v,
        Err(e) => {
            // this may not be the best option - returning an error may be more appropriate
            panic!("Failed to gzip the payload: {}", e);
        }
    };

    // encode to base58
    let response = bs58::encode(&gzipped).into_string();

    info!("Compressed: {}, encoded: {}", compressed_len, response.len());

    response
}
