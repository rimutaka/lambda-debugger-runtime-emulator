# Lambda Proxy Function for Remote Debugging

This lambda function sends both `Payload` and `Context` to a predefined SQS queue and then waits for a response that is deserialized and passed back onto the runtime.

In the meantime, you can read the message from the queue, deserialize  `Payload` and `Context`, process it locally and send back a response. If your local lambda fails you can re-read the message until either the proxy lambda times out or you send back a reply.

The benefit of this approach vs using a local mock environment is the full integration in your infra. It is not always possible to use mock input or try to copy the input by hand. The proxy function will give you near-real time request/response with real data.

### Deployment

Replace `--region ap-southeast-2` and `--function-name lambda-debug-proxy` with your values.

```
cargo build --release --target x86_64-unknown-linux-musl

cp ./target/x86_64-unknown-linux-musl/release/lambda-debug-proxy ./bootstrap && zip proxy.zip bootstrap && rm bootstrap

aws lambda update-function-code --region ap-southeast-2 --function-name lambda-debug-proxy --zip-file fileb://proxy.zip
```

### Lambda config

Create an empty Lambda function with `Custom runtime on Amazon Linux 2` and run the deployment script from the previous section. It will build and upload the code to AWS.

Recommended settings:

- **Runtime**: Custom runtime on Amazon Linux 2
- **Memory (MB)**: 128
- **Timeout**: 15min 0sec
- **Reserved concurrency**: 1
- **Maximum age of event**: 0h 1min 0sec
- **Retry attempts**: 0


#### Env variables
- `LAMBDA_PROXY_TRACING_LEVEL` - optional, default=INFO, use DEBUG to get full lambda logging or TRACE to go deeper into dependencies.
- `AWS_DEFAULT_REGION` or `AWS_REGION` - required, but they should be pre-set by AWS
- `LAMBDA_PROXY_REQ_QUEUE_URL` - the Queue URL for Lambda requests, required
- `LAMBDA_PROXY_RESP_QUEUE_URL` - the Queue URL for Lambda responses, required

### Queue config

Create `LAMBDA_PROXY_REQ` and `LAMBDA_PROXY_RESP` SQS queues. Allow r/w access to both from the Lambda and Client IAM Roles.

Recommended settings:

- **Type**: Standard
- **Maximum message size**: 256 KB
- **Default visibility timeout**: 10 Seconds
- **Message retention period**: 1 Hour
- **Receive message wait time**: 20 Seconds

### Client config

The lambda code running on your local machine has to be wrapped into a client that reads the queue, de-serializes the payload into your required format, calls your lambda and sends the response back. It is the exact reverse of what the  proxy function does. See [local.rs example](examples/local.rs) for a sample Rust implementation. It should be easy to port the sample wrapper into the language of your Lambda handler. There is no need to refactor the proxy.

#### Running the Rust client from `examples/local.rs`:
```
cargo run --example local
```
