# Lambda Runtime Emulator for local debugging

This runtime emulator allows debugging AWS Lambda functions written in Rust locally while receiving the payload from AWS and sending the responses back as if you were doing remote debugging inside the AWS environment.


## How it works

This project has two crates:

__Production configuration__

Consider this typical Lambda use case:

![function needed debugging](./img/lambda-debugger-usecase.png)

__Debugging configuration__

- _proxy-lambda_ crate forwards Lambda requests and responses between AWS and your development machine in real time
- _runtime-emulator_ crate provides necessary APIs and exchanges payloads with _proxy-lambda_ to enable your Lambda function to run locally

![function debugged locally](./img/lambda-debugger-components.png)

## Getting started

__Initial setup:__

- clone this repository locally
- build for release or debug
- create _request_ and _response_ queues in SQS with IAM permissions

__Per Lambda function:__

- deploy _proxy-lambda_ in place of the function you need to debug
- run the emulator in the terminal on the local machine as a binary or with `cargo run`
- run your lambda locally with `cargo run`



Detailed instructions and code samples for the above steps are provided further in this document.

### Limitations

This emulator provides the necessary API endpoints for the lambda function to run. It does not:

* constrain the environment, e.g. memory or execution time
* report errors back to AWS
* handle concurrent requests
* copies the entire set of env vars from AWS (see [runtime-emulator/env-template.sh](runtime-emulator/env-template.sh))
* no support for X-Trace or Extensions APIs

If the local lambda sends back the response before other services time out (e.g. API GW times out after 30s)
the _proxy_lambda_ returns it to the caller.

## Deployment in details

### SQS configuration

Create `PROXY_LAMBDA_REQ` and `PROXY_LAMBDA_RESP` SQS queues.
You may use different names, but it is easier to use the defaults provided here.

Recommended settings:

- **Queue type**: Standard
- **Maximum message size**: 256 KB
- **Default visibility timeout**: 10 Seconds
- **Message retention period**: 1 Hour
- **Receive message wait time**: 20 Seconds

This IAM policy grants _proxy-lambda_ access to the queues.
It assumes that you have sufficient privileges to access Lambda and SQS from your local machine.

Replace _Principal_ and _Resource_ IDs with your values before adding this policy to the queue config.

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "AWS": "arn:aws:iam::512295225992:role/lambda_basic"
      },
      "Action": [
        "sqs:DeleteMessage",
        "sqs:GetQueueAttributes",
        "sqs:ReceiveMessage",
        "sqs:SendMessage"
      ],
      "Resource": "arn:aws:sqs:us-east-1:512295225992:PROXY_LAMBDA_REQ"
    }
  ]
}
```


### Building and deploying _proxy-lambda_

The _proxy lambda_ function should be deployed to AWS Lambda in place of the function you want to debug.

Replace the following parts of the snippet with your values before running it from the project root:
- _target_ - the architecture of the lambda function on AWS, e.g. `x86_64-unknown-linux-gnu`
- _region_ - the region of the lambda function, e.g. `us-east-1`
- _name_ - the name of the lambda function you want to replace with the proxy, e.g. `my-lambda`

```
target=x86_64-unknown-linux-gnu 
region=us-east-1
name=my-lambda

cargo build --release --target $target
cp ./target/$target/release/proxy-lambda ./bootstrap && zip proxy.zip bootstrap && rm bootstrap
aws lambda update-function-code --region $region --function-name $name --zip-file fileb://proxy.zip
```

A deployed _proxy-lambda_ should return _Config error_ if you run it with the test event from the console.



### Lambda environmental variables

- `PROXY_LAMBDA_TRACING_LEVEL` - optional, default=INFO, use DEBUG to get full lambda logging or TRACE to go deeper into dependencies.
- `AWS_DEFAULT_REGION` or `AWS_REGION` - required, but they should be pre-set by AWS
- `LAMBDA_PROXY_REQ_QUEUE_URL` - the Queue URL for Lambda proxy requests, required
- `LAMBDA_PROXY_RESP_QUEUE_URL` - the Queue URL for Lambda handler responses, optional.

The proxy runs asynchronously if no `LAMBDA_PROXY_RESP_QUEUE_URL` is specified. It sends the request to `LAMBDA_PROXY_REQ_QUEUE_URL` and returns `OK` regardless of what happens at the remote handler's end.
This is useful for debugging asynchronous functions like S3 event handlers. 

## Debugging

Pre-requisites:
- _proxy-lambda_ was deployed and configured
- SQS queues were created with the appropriate access policies
- modify [runtime-emulator/env-minimal.sh](runtime-emulator/env-minimal.sh) with your IDs

__Launching the emulator:__
- run the modified [runtime-emulator/env-minimal.sh](runtime-emulator/env-minimal.sh) in a terminal window on your local machine
- start the _runtime-emulator_ in the same terminal window as a binary or with `cargo run`
- the app should inform you it is listening on a certain port

__Launching the local lambda:__
- run the modified [runtime-emulator/env-minimal.sh](runtime-emulator/env-minimal.sh) in a terminal window on your local machine
- start your lambda in the same terminal window with `cargo run`
- the emulator will inform you it is waiting for an incoming message from the SQS

__Debugging:__
- trigger the event on AWS as part of your normal data flow, e.g. by a user action on a webpage
- the emulator should display the lambda payload and forward it to your local lambda for processing
- debug
- successful responses are sent back to the caller if the response queue is configured 

If the local lambda fails, terminates or panics, you can make changes to the code and run it again to reuse the same payload.


## Technical details

### SQS

If you are not familiar with [AWS SQS](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/welcome.html) you may not know that the messages [have to be explicitly deleted](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/APIReference/API_DeleteMessage.html) from the queue. The request messages are deleted by the handler wrapper when the handler returns a response. This allows re-running the handler if it fails before sending a response, which is a handy debugging feature. The response messages are deleted by the handler proxy as soon as they arrive.

It is possible for the response to arrive too late because either the Lambda Runtime or the caller timed out. For example, AWS APIGateway wait is limited to 30s. The Lambda function can be configured to wait for up to 15 minutes. Remember to check that all stale messages got deleted and purge the queues via the console or AWS CLI if needed. 

### Large payloads and data compression

The proxy Lambda function running on AWS and the client running on the dev's machine send JSON payload to SQS. The size of the payload is [limited to 262,144 bytes by SQS](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/quotas-messages.html). To get around this limitation the client may compress the JSON payload using [flate2 crate](https://crates.io/crates/flate2) and send it as an encoded Base58 string. The encoding/decoding happens automatically at both ends. Only large messages are compressed because it takes up to a few seconds in debug mode.
