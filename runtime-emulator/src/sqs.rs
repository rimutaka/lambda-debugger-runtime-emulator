use crate::config;
use aws_sdk_sqs::{types::Message, Client as SqsClient};
use flate2::read::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::OnceCell;
use std::io::prelude::*;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};
use uuid::Uuid;

/// An internal version of Lambda Context struct, similar to the one in the lambda_runtime crate.
#[derive(Deserialize, Debug, Serialize)]
#[serde(default)]
pub(crate) struct Ctx {
    /// From the context
    pub request_id: String,
    /// From the context
    pub deadline: i64,
    /// From the context
    pub invoked_function_arn: String,
    /// From the context
    pub xray_trace_id: String,
    // Rust runtime provides a comprehensive Context struct, but it has no default implementation
    // and lots of extra fields. It was easier to implement our own.
}

// This is probably redundant as we should always have the context values coming from Lambda.
impl Default for Ctx {
    fn default() -> Self {
        Ctx {
            request_id: Uuid::new_v4().to_string(),
            deadline: 2524608000,
            invoked_function_arn: "my-lambda".to_owned(),
            xray_trace_id:
                "Root=0-00000000-000000000000000000000000;Parent=0000000000000000;Sampled=0;Lineage=00000000:0"
                    .to_owned(),
        }
    }
}

/// A local implementation of lambda_runtime::LambdaEvent<T>
#[derive(Deserialize, Debug, Serialize)]
pub(crate) struct RequestPayload {
    pub event: Value, // using Value to extract some fields and pass the rest to the runtime
    pub ctx: Ctx,
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
    let cell = OnceCell::new();
    let config = cell.get_or_init(config::Config::from_env);

    let client = SqsClient::new(&aws_config::load_from_env().await);

    // start listening to the response
    loop {
        // try to get the next message and wait for it to arrive if none is ready
        // sleep for a bit on error before retrying
        let resp = match client
            .receive_message()
            .max_number_of_messages(1)
            .set_queue_url(Some(config.request_queue_url.clone()))
            .set_wait_time_seconds(Some(20))
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

        // let pld = pld.event.as_str().expect("event is not &str");
        info!("Value: {payload}");

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

/// Send back the response and delete the message from the queue.
pub(crate) async fn send_output(response: String, receipt_handle: String) {
    let cell = OnceCell::new();
    let config = cell.get_or_init(config::Config::from_env);

    let client = SqsClient::new(&aws_config::load_from_env().await);

    let response = compress_output(response);

    // SQS messages must be shorter than 262144 bytes
    if response.len() < 262144 {
        if let Err(e) = client
            .send_message()
            .set_message_body(Some(response))
            .set_queue_url(Some(config.response_queue_url.to_string()))
            .send()
            .await
        {
            panic!("Failed to send SQS response: {}", e);
        };
    } else {
        info!(
            "Message size is too big for SQS: {}B, max allowed: 262,144 bytes",
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
}

/// Compresses and encodes the output as Base58 if the message is larger than what is
/// allowed in SQS (262,144 bytes)
fn compress_output(response: String) -> String {
    // is it small enough to fit in?
    if response.len() < 262144 {
        return response;
    }

    println!(
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

// /// A standard routine for initializing a tracing provider for use in `main` and inside test functions.
// /// * tracing_level: pass None if not known in advance and should be taken from an env var
// pub fn init_tracing(tracing_level: Option<tracing::Level>) {
//     // get the log level from an env var
//     let tracing_level = match tracing_level {
//         Some(v) => v,
//         None => match var("LAMBDA_PROXY_TRACING_LEVEL") {
//             Err(_) => tracing::Level::INFO,
//             Ok(v) => tracing::Level::from_str(&v).expect("Invalid tracing level. Use trace, debug, error or info"),
//         },
//     };

//     // init the logger with the specified level
//     tracing_subscriber::fmt()
//         .with_max_level(tracing_level)
//         .with_ansi(false)
//         .without_time()
//         .init();
// }

// const AWS_REGION: Region = Region::UsEast1; // replace with your preferred region
// const REQUEST_QUEUE_URL_ENV: &str = "STM_HTML_LAMBDA_PROXY_REQ"; // add your queue URL there
// const RESPONSE_QUEUE_URL_ENV: &str = "STM_HTML_LAMBDA_PROXY_RESP"; // add your queue URL there
