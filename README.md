# Lambda Proxy Function for Remote Debugging

This lambda function sends both `Payload` and `Context` to a predefined SQS queue and then waits for a response that is deserialized and passed back onto the runtime.

In the meantime, you can read the message from the queue, deserialize  `Payload` and `Context`, process it locally and send back a response. If your local lambda fails you can re-read the message until either the proxy lambda times out or you send back a reply.

The benefit of this approach vs using a local mock environment is the full integration in your infra. It is not always possible to use mock input or try to copy the input by hand. The proxy function will give you near-real time request/response with real data.

### Lambda config

#### Env variables
- `LAMBDA_PROXY_TRACING_LEVEL` - optional, default=INFO, use DEBUG to get full lambda logging or TRACE to go deeper into dependencies.
- `AWS_DEFAULT_REGION` or `AWS_REGION` - required
- `LAMBDA_PROXY_REQ_QUEUE_URL` - the Queue URL for Lambda requests, required
- `LAMBDA_PROXY_RESP_QUEUE_URL` - the Queue URL for Lambda responses, required

### Queue config

...

### Client config

The lambda code running on your local machine has to be wrapped into a client that reads the queue, deserializes the payload into your required format, calls your lambda and sends the response back. It is the exact reverse of what this proxy does. See ... example for a Rust implementation. You can implement it in the language of your lambda, e.g. C#. 