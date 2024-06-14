use lambda_runtime::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A local implementation of lambda_runtime::LambdaEvent<T>.
/// It replicates LambdaEvent<Value> because we need Ser/Deser traits not implemented for LambdaEvent.
#[derive(Deserialize, Debug, Serialize)]
pub struct RequestPayload {
    pub event: Value, // using Value to extract some fields and pass the rest to the runtime
    pub ctx: Context,
}
