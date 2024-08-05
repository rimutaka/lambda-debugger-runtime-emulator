/// This is a basic lambda for testing the emulator locally.
use lambda_runtime::{service_fn, Error, LambdaEvent, Runtime};
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
    // minimal logging to keep it simple
    // intended to run locally only
    tracing_subscriber::fmt()
        .without_time()
        .with_ansi(true) // the color codes work in the terminal only
        .with_target(false)
        .init();

    // init the runtime directly to avoid the extra logging layer
    let runtime = Runtime::new(service_fn(my_handler));
    runtime.run().await?;

    Ok(())
}

pub(crate) async fn my_handler(event: LambdaEvent<Request>) -> Result<Response, Error> {
    info!("Handler invoked");

    let command = event.payload.command;

    info!("Command received: {}", command);

    Ok(Response {
        req_id: event.context.request_id,
        msg: "Hello from Rust!".to_string(),
    })

    // Err(Error::from("Error"))
}
