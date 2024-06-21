target=x86_64-unknown-linux-gnu 
region=us-east-1
name=my-lambda

cargo build --release --target $target
cp ./target/$target/release/proxy-lambda ./bootstrap && zip proxy.zip bootstrap && rm bootstrap
aws lambda update-function-code --region $region --function-name $name --zip-file fileb://proxy.zip