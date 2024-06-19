/// This is a basic lambda for testing the emulator locally. 

use lambda_runtime::{service_fn, tracing, Error, LambdaEvent};
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Deserialize, Debug)]
struct Request {
    command: String,
}

#[derive(Serialize)]
struct Response {
    req_id: String,
    msg: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // required to enable CloudWatch error logging by the runtime
    tracing::init_default_subscriber();

    let func = service_fn(my_handler);
    lambda_runtime::run(func).await?;
    Ok(())
}

pub(crate) async fn my_handler(event: LambdaEvent<Request>) -> Result<Response, Error> {
    info!("Received event: {:?}", event);

    // extract some useful info from the request
    let command = event.payload.command;

    info!("Command received: {}", command);

    // prepare the response
    let resp = Response {
        req_id: event.context.request_id,
        msg: format!("Command {} executed.", command),
    };

    // return `Response` (it will be serialized to JSON automatically by the runtime)
    Ok(resp)
}
