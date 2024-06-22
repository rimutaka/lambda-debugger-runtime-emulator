# Run this script in the terminal windows for the runtime emulator and the lambda you are testing
# or set them globally by running it at the system startup.

# These queues is how the emulator communicates with the proxy lambda on AWS
# Replace with your own queue URLs
export PROXY_LAMBDA_REQ_QUEUE_URL=https://sqs.us-east-1.amazonaws.com/512295225992/proxy_lambda_req,
# The response queue can be omitted if you don't need to forward responses to the caller
export PROXY_LAMBDA_RESP_QUEUE_URL=https://sqs.us-east-1.amazonaws.com/512295225992/proxy_lambda_resp

# These vars are needed by the lambda function you are testing
# Replace with your values, if needed
export AWS_LAMBDA_FUNCTION_VERSION=$LATEST
export AWS_LAMBDA_FUNCTION_MEMORY_SIZE=128
export AWS_LAMBDA_FUNCTION_NAME=lambda-debug-proxy

# Leave the AWS default (127.0.0.1:9001) unless you have to change it
# It tells the lambda function where the runtime emulator is
export AWS_LAMBDA_RUNTIME_API=127.0.0.1:9001

