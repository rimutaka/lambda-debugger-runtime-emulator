# Run this script in the terminal windows for the runtime emulator
# if you need to provide custom values for queue URLs or the runtime API address.
# Set them globally by running it at the system startup, e.g. in .bashrc for repeated use.

# These queues is how the emulator communicates with the proxy lambda on AWS
# Replace with your own queue URLs if they are different from the default
export PROXY_LAMBDA_REQ_QUEUE_URL=https://sqs.us-east-1.amazonaws.com/[your_aws_account]/proxy_lambda_req,
# The response queue can be omitted if you don't need to forward responses to the caller
export PROXY_LAMBDA_RESP_QUEUE_URL=https://sqs.us-east-1.amazonaws.com/[your_aws_account]/proxy_lambda_resp

# Leave the AWS default (127.0.0.1:9001) unless you have to change it
# It tells the lambda function where the runtime emulator is
export AWS_LAMBDA_RUNTIME_API=127.0.0.1:9001

