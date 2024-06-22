use aws_sdk_sqs::Client as SqsClient;
use flate2::read::GzDecoder;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use runtime_emulator_types::RequestPayload;
use serde_json::Value;
use std::env::var;
use std::io::Read;
use std::str::FromStr;
use tracing::{debug, error, info};

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::filter::EnvFilter::from_default_env())
        .with_ansi(false)
        .without_time()
        .compact()
        .init();

    print_env_vars();

    if let Err(e) = lambda_runtime::run(service_fn(my_handler)).await {
        debug!("Runtime error: {:?}", e);
        return Err(Error::from(e));
    }

    Ok(())
}

async fn my_handler(event: LambdaEvent<Value>) -> Result<Value, Error> {
    let (event, ctx) = event.into_parts();

    debug!("Event: {:?}", event);
    debug!("Context: {:?}", ctx);

    // to be used a few times later
    let invoked_function_arn = ctx.invoked_function_arn.clone();

    // check if the request queue URL was specified via an env var
    // if not, use the default queue URL
    let request_queue_url = match var("PROXY_LAMBDA_REQ_QUEUE_URL") {
        Ok(v) => v,
        Err(_e) => {
            // the env var does not exist - try to use the default queue URL
            // there shouldn't be any other env var errors, so the error type can be ignored
            let arn = invoked_function_arn.split(':').collect::<Vec<&str>>();
            // arn example: arn:aws:lambda:us-east-1:512295225992:function:my-lambda
            if arn.len() != 7 {
                error!(
                    "ARN should have 7 parts, but it has {}: {}",
                    arn.len(),
                    ctx.invoked_function_arn
                );
                return Err(Error::from("Context error"));
            }

            debug!(
                "Sending to default proxy_lambda_req queue name. Use PROXY_LAMBDA_REQ_QUEUE_URL env var to specify a different queue."
            );

            // example: https://sqs.us-east-1.amazonaws.com/512295225992/proxy_lambda_req
            format!("https://sqs.{}.amazonaws.com/{}/proxy_lambda_req", arn[3], arn[4])
        }
    };

    debug!("ReqQ URL: {}", request_queue_url);

    let client = SqsClient::new(&aws_config::load_from_env().await);

    // Sending part
    let request_payload = RequestPayload { event, ctx };

    let message_body = match serde_json::to_string(&request_payload) {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to serialize event + context: {:?}", e);
            return Err(Error::from(e));
        }
    };

    debug!("Message body: {}", message_body);

    let send_result = match client
        .send_message()
        .set_message_body(Some(message_body))
        .set_queue_url(Some(request_queue_url.to_string()))
        .send()
        .await
    {
        Ok(v) => v,
        Err(e) => {
            debug!("Error sending message: {:?}", e);
            return Err(Error::from("Failed to send message"));
        }
    };

    let msg_id = send_result.message_id.unwrap_or_default();
    debug!("Sent with ID: {}", msg_id);

    // This proxy should wait for a response from the local lambda if there is a response queue.
    // To determine if there is a response queue the proxy checks for the env var and tries to purge it.
    // If no env var is set, the proxy tries to purge the default queue.
    // Exit with OK if the env var does not exist and the default queue does not exist or gives this lambda no access
    let response_queue_url = match var("PROXY_LAMBDA_RESP_QUEUE_URL") {
        Ok(response_queue_url) => {
            debug!("RespQ URL from env var: {}", response_queue_url);
            // clear the response queue to avoid getting a stale message from a previously timed out request
            purge_response_queue(&client, &response_queue_url).await?;
            response_queue_url
        }
        Err(_) => {
            // queue env var does not exist - try to construct the default queue URL out of the lambda ARN
            let arn = invoked_function_arn.split(':').collect::<Vec<&str>>();
            // arn example: arn:aws:lambda:us-east-1:512295225992:function:my-lambda

            if arn.len() != 7 {
                error!(
                    "ARN should have 7 parts, but it has {}: {}",
                    arn.len(),
                    invoked_function_arn
                );
                return Err(Error::from("Context error"));
            }

            // sample SQS URL https://sqs.us-east-1.amazonaws.com/512295225992/proxy_lambda_resp
            let response_queue_url = format!("https://sqs.{}.amazonaws.com/{}/proxy_lambda_resp", arn[3], arn[4]);

            debug!("RespQ URL from default: {}", response_queue_url);
            debug!("Use PROXY_LAMBDA_RESP_QUEUE_URL env var to specify a different queue");

            // if this call fails it may mean the queue does not exist or is misconfigured
            // take this as the signal to not wait for a response
            if let Err(_e) = purge_response_queue(&client, &response_queue_url).await {
                debug!("Configure PROXY_LAMBDA_RESP_QUEUE_URL env var the default queue to wait for responses.");
                return Ok(Value::Null);
            };

            response_queue_url
        }
    };

    // wait the response until one arrives or the lambda times out
    loop {
        debug!("20s loop");
        let resp = match client
            .receive_message()
            .max_number_of_messages(1)
            .set_queue_url(Some(response_queue_url.to_string()))
            .set_wait_time_seconds(Some(20))
            .send()
            .await
        {
            Ok(v) => v,
            Err(e) => {
                debug!("Error receiving messages: {:?}", e);
                return Err(Error::from("Failed to receive messages"));
            }
        };

        // wait until a message arrives or the function is killed by AWS
        // an empty list returns when the queue wait time expires
        let mut msgs = match resp.messages {
            Some(v) => v,
            None => {
                debug!("No messages yet: message list is None");
                continue;
            }
        };
        if msgs.is_empty() {
            debug!("No messages yet: empty message list");
            continue;
        } else {
            debug!("Received {} messages", msgs.len());
        }

        // message arrived - grab its handle for future reference
        let receipt_handle = match msgs[0].receipt_handle.as_ref() {
            Some(v) => v,
            None => {
                return Err(Error::from("Failed to get msg receipt"));
            }
        }
        .to_owned();

        let body = match match msgs.pop() {
            Some(v) => v,
            None => {
                return Err(Error::from(
                    "msgs Vec should have been pre-checked for is_empty(). It's a bug.",
                ));
            }
        }
        .body
        {
            Some(v) => v,
            None => {
                return Err(Error::from("Failed to get message body"));
            }
        };

        debug!("Response:{}", body);

        let body = decode_maybe_binary(body)?;

        // delete it from the queue so it's not picked up again
        match client
            .delete_message()
            .set_queue_url(Some(response_queue_url.to_string()))
            .set_receipt_handle(Some(receipt_handle))
            .send()
            .await
        {
            Ok(v) => v,
            Err(e) => {
                debug!("Error deleting messages: {:?}", e);
                return Err(Error::from("Error deleting messages"));
            }
        };
        debug!("Message deleted");

        // return the contents of the message as JSON Value
        return Ok(Value::from_str(&body)?);
    }
}

/// Checks if the message is a Base58 encoded compressed text and either decodes/decompresses it
/// or returns as-is if it's not encoded/compressed.
fn decode_maybe_binary(body: String) -> Result<String, Error> {
    // check for presence of { at the beginning of the doc to determine if it's JSON or Base58
    if body.is_empty() || body.trim_start().starts_with('{') {
        // looks like JSON - return as-is
        return Ok(body);
    }

    // try to decode base58
    let body_decoded = match bs58::decode(&body).into_vec() {
        Ok(v) => v,
        Err(e) => {
            debug!("Failed to decode from maybe base58: {:?}", e);
            return Err(Error::from("Failed to decode from maybe base58"));
        }
    };

    // try to decompress the body
    let mut decoder = GzDecoder::new(body_decoded.as_slice());
    let mut decoded: Vec<u8> = Vec::new();
    let len = match decoder.read_to_end(&mut decoded) {
        Ok(v) => v,
        Err(e) => {
            debug!("Failed to decompress the payload: {:?}", e);
            return Err(Error::from("Failed to decompress the payload"));
        }
    };

    debug!("Decoded {} bytes", len);

    // return the bytes converted into a lossy unicode string or an error
    match String::from_utf8(decoded) {
        Ok(v) => Ok(v),
        Err(e) => {
            debug!("Failed to convert decompressed payload to UTF8: {:?}", e);
            Err(Error::from("Failed to convert decompressed payload to UTF8"))
        }
    }
}

async fn purge_response_queue(client: &SqsClient, response_queue_url: &str) -> Result<(), Error> {
    debug!("Purging the queue, one msg at a time.");
    loop {
        let resp = match client
            .receive_message()
            .max_number_of_messages(10)
            .set_queue_url(Some(response_queue_url.to_string()))
            .set_wait_time_seconds(Some(0))
            .send()
            .await
        {
            Ok(v) => v,
            Err(e) => {
                debug!("Error receiving messages: {:?}", e);
                return Err(Error::from("Error receiving messages"));
            }
        };

        // an empty list returns when the queue wait time expires
        let msgs = match resp.messages {
            Some(v) => v,
            None => {
                debug!("No stale messages in purge queue");
                return Ok(());
            }
        };

        if msgs.is_empty() {
            debug!("No stale messages (resp.messages.is_empty)");
            return Ok(());
        }

        debug!("Deleting {} stale messages", msgs.len());

        for msg in msgs {
            // delete it from the queue
            match client
                .delete_message()
                .set_queue_url(Some(response_queue_url.to_string()))
                .set_receipt_handle(msg.receipt_handle)
                .send()
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    debug!("Error deleting messages: {:?}", e);
                    return Err(Error::from("Error deleting messages"));
                }
            };
            debug!("Message deleted");
        }
    }
}

/// Prints all environment variables to the log in the form of `export KEY=VALUE key2=value2`
fn print_env_vars() {
    let mut env_vars = Vec::<String>::with_capacity(30);
    env_vars.push(" export".to_string()); // the space at the front is needed to keep EXPORT as the first item of the array
    for (key, value) in std::env::vars() {
        match key.as_str() {
            "AWS_ACCESS_KEY_ID" | "AWS_SECRET_ACCESS_KEY" | "AWS_SESSION_TOKEN" => {
                // do not log sensitive vars
            }
            _ => {
                env_vars.push(format!("{}={}", key, value));
            }
        }
    }

    // the list is easier to deal with when sorted
    env_vars.sort();

    info!("{}", env_vars.join(" "));
}
