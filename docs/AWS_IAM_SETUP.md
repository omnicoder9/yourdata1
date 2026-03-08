# AWS IAM & Environment Setup for Consent-Gated Ingress

This guide covers everything needed to configure your local machine and AWS account before running the deploy script or integration tests.

---

## 1. Prerequisites

- AWS CLI v2 installed (`aws --version`)
- An AWS account with admin access (or scoped permissions below)
- Rust toolchain (`rustc`, `cargo` — either via `rustup` or system package manager)
- Python 3 with `venv` module (for cargo-lambda auto-install)
- `cargo-lambda` — the deploy script auto-installs this into `~/.local/share/cargo-lambda-venv` and symlinks it into `~/.local/bin` if not found. To install manually: `pip install cargo-lambda` or `cargo install cargo-lambda`

---

## 2. AWS credentials on your machine

### Option A: IAM user with access keys (dev/test)

1. In the AWS Console, go to **IAM > Users > Create user**.
2. Name it something like `yourdata-deployer`.
3. On the **Set permissions** step, attach the managed policy `AdministratorAccess` for initial setup, or use the scoped policy in section 4 below.
4. After creation, go to the user's **Security credentials** tab and create an access key (choose "Command Line Interface" use case).
5. Configure locally:

```bash
aws configure
# AWS Access Key ID:     <paste>
# AWS Secret Access Key: <paste>
# Default region name:   us-east-1
# Default output format: json
```

This writes to `~/.aws/credentials` and `~/.aws/config`.

### Option B: IAM Identity Center / SSO (recommended for teams)

```bash
aws configure sso
# SSO session name: yourdata
# SSO start URL:    https://<your-org>.awsapps.com/start
# SSO region:       us-east-1
# Select account and role when prompted
```

Then set the profile before running commands:

```bash
export AWS_PROFILE=yourdata-sso-profile
```

### Option C: Temporary session credentials (CI or short-lived)

If you have an IAM role ARN and MFA:

```bash
aws sts assume-role \
  --role-arn arn:aws:iam::123456789012:role/YourDeployRole \
  --role-session-name deploy-session \
  --serial-number arn:aws:iam::123456789012:mfa/your-user \
  --token-code 123456
```

Export the returned credentials:

```bash
export AWS_ACCESS_KEY_ID=<AccessKeyId>
export AWS_SECRET_ACCESS_KEY=<SecretAccessKey>
export AWS_SESSION_TOKEN=<SessionToken>
```

### Verify credentials work

```bash
aws sts get-caller-identity
```

You should see your account ID, user/role ARN, and user ID.

---

## 3. Environment variables

### Required for deploy (`scripts/deploy-aws.sh`)

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `AWS_ACCESS_KEY_ID` | Yes (unless using `~/.aws/credentials` or SSO) | — | IAM access key |
| `AWS_SECRET_ACCESS_KEY` | Yes (unless using `~/.aws/credentials` or SSO) | — | IAM secret key |
| `AWS_SESSION_TOKEN` | Only for assumed roles | — | Temporary session token |
| `AWS_REGION` | No | `us-east-1` | Target region for all resources |

The deploy script also accepts `--region` and `--prefix` flags which override `AWS_REGION` and the resource name prefix (`yourdata` by default).

### Required for the Lambda function at runtime

These are set automatically by `deploy-aws.sh` as Lambda environment variables. You do not need to set them locally unless running the binary outside Lambda.

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `CONSENT_TABLE_NAME` | No | `yourdata-consents` | DynamoDB table name |
| `EVENT_QUEUE_URL` | **Yes** | — | Full SQS queue URL |
| `KMS_KEY_ID` | No | — | KMS key ID or alias for field encryption. If unset, sensitive field encryption is disabled. |
| `RUST_LOG` | No | `info` | Log level filter (`debug`, `info`, `warn`, `error`) |

### Required for integration tests

Set these before running `INTEGRATION=1 cargo test --test integration_tests`:

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `INTEGRATION` | **Yes** | — | Must be `1` to enable integration tests |
| `AWS_REGION` | Yes | — | Region where test resources exist |
| `CONSENT_TABLE_NAME` | No | `yourdata-consents` | DynamoDB table for test consent records |
| `EVENT_QUEUE_URL` | **Yes** | — | SQS queue URL |
| `KMS_KEY_ID` | No | — | Set to run KMS encryption tests; omit to skip them |
| `LOCALSTACK_ENDPOINT` | No | — | Set to e.g. `http://localhost:4566` to test against LocalStack instead of real AWS |

Example:

```bash
export AWS_REGION=us-east-1
export CONSENT_TABLE_NAME=yourdata-consents
export EVENT_QUEUE_URL=https://sqs.us-east-1.amazonaws.com/123456789012/yourdata-events-queue
export KMS_KEY_ID=alias/yourdata-field-encryption

cd lambda/consent-ingress
INTEGRATION=1 cargo test --test integration_tests
```

---

## 4. IAM permissions

### 4a. Deployer permissions (your local user/role)

The user or role running `deploy-aws.sh` and `teardown-aws.sh` needs broad provisioning access. You can use `AdministratorAccess` for a personal dev account, or scope it down with this custom policy:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "DynamoDB",
      "Effect": "Allow",
      "Action": [
        "dynamodb:CreateTable",
        "dynamodb:DeleteTable",
        "dynamodb:DescribeTable",
        "dynamodb:PutItem",
        "dynamodb:GetItem",
        "dynamodb:DeleteItem"
      ],
      "Resource": "arn:aws:dynamodb:*:*:table/yourdata-*"
    },
    {
      "Sid": "SQS",
      "Effect": "Allow",
      "Action": [
        "sqs:CreateQueue",
        "sqs:DeleteQueue",
        "sqs:GetQueueUrl",
        "sqs:GetQueueAttributes",
        "sqs:PurgeQueue",
        "sqs:SendMessage",
        "sqs:ReceiveMessage"
      ],
      "Resource": "arn:aws:sqs:*:*:yourdata-*"
    },
    {
      "Sid": "KMS",
      "Effect": "Allow",
      "Action": [
        "kms:CreateKey",
        "kms:CreateAlias",
        "kms:DeleteAlias",
        "kms:ScheduleKeyDeletion",
        "kms:CancelKeyDeletion",
        "kms:ListAliases",
        "kms:Encrypt",
        "kms:DescribeKey"
      ],
      "Resource": "*"
    },
    {
      "Sid": "Lambda",
      "Effect": "Allow",
      "Action": [
        "lambda:CreateFunction",
        "lambda:UpdateFunctionCode",
        "lambda:UpdateFunctionConfiguration",
        "lambda:DeleteFunction",
        "lambda:GetFunction",
        "lambda:AddPermission",
        "lambda:RemovePermission"
      ],
      "Resource": "arn:aws:lambda:*:*:function:yourdata-*"
    },
    {
      "Sid": "APIGateway",
      "Effect": "Allow",
      "Action": [
        "apigateway:POST",
        "apigateway:GET",
        "apigateway:PUT",
        "apigateway:DELETE"
      ],
      "Resource": "arn:aws:apigateway:*::/*"
    },
    {
      "Sid": "IAMForLambdaRole",
      "Effect": "Allow",
      "Action": [
        "iam:CreateRole",
        "iam:DeleteRole",
        "iam:GetRole",
        "iam:PutRolePolicy",
        "iam:DeleteRolePolicy",
        "iam:PassRole"
      ],
      "Resource": "arn:aws:iam::*:role/yourdata-*"
    },
    {
      "Sid": "CloudWatchLogs",
      "Effect": "Allow",
      "Action": [
        "logs:CreateLogGroup",
        "logs:DeleteLogGroup"
      ],
      "Resource": "arn:aws:logs:*:*:log-group:/aws/lambda/yourdata-*"
    },
    {
      "Sid": "SecretsManager",
      "Effect": "Allow",
      "Action": [
        "secretsmanager:CreateSecret",
        "secretsmanager:DeleteSecret",
        "secretsmanager:DescribeSecret"
      ],
      "Resource": "arn:aws:secretsmanager:*:*:secret:yourdata/*"
    },
    {
      "Sid": "STSIdentity",
      "Effect": "Allow",
      "Action": "sts:GetCallerIdentity",
      "Resource": "*"
    }
  ]
}
```

To create this as a managed policy:

```bash
aws iam create-policy \
  --policy-name yourdata-deployer-policy \
  --policy-document file://docs/deployer-policy.json
```

Then attach it to your IAM user or role:

```bash
aws iam attach-user-policy \
  --user-name yourdata-deployer \
  --policy-arn arn:aws:iam::123456789012:policy/yourdata-deployer-policy
```

### 4b. Lambda execution role (created by deploy script)

The deploy script creates `yourdata-consent-ingress-role` automatically with an inline policy scoped to exactly these actions:

| Action | Resource | Purpose |
|--------|----------|---------|
| `dynamodb:GetItem` | `yourdata-consents` table | Read consent records |
| `sqs:SendMessage` | `yourdata-events-queue` | Enqueue accepted events |
| `kms:Encrypt` | The specific KMS key | Encrypt sensitive payload fields |
| `logs:CreateLogGroup`, `logs:CreateLogStream`, `logs:PutLogEvents` | `/aws/lambda/yourdata-consent-ingress` | CloudWatch logging |

The role's trust policy allows only `lambda.amazonaws.com` to assume it. No human or other service can use it.

---

## 5. Manual security configurations

### 5a. KMS key policy

The deploy script creates a KMS key with the default key policy (account root has full access). For production, restrict the key policy so only the Lambda role can encrypt and only designated decryptor roles can decrypt:

```bash
aws kms put-key-policy \
  --key-id <KMS_KEY_ID> \
  --policy-name default \
  --region us-east-1 \
  --policy '{
    "Version": "2012-10-17",
    "Statement": [
      {
        "Sid": "RootAdmin",
        "Effect": "Allow",
        "Principal": {"AWS": "arn:aws:iam::123456789012:root"},
        "Action": "kms:*",
        "Resource": "*"
      },
      {
        "Sid": "LambdaEncrypt",
        "Effect": "Allow",
        "Principal": {"AWS": "arn:aws:iam::123456789012:role/yourdata-consent-ingress-role"},
        "Action": "kms:Encrypt",
        "Resource": "*"
      },
      {
        "Sid": "WorkerDecrypt",
        "Effect": "Allow",
        "Principal": {"AWS": "arn:aws:iam::123456789012:role/yourdata-worker-role"},
        "Action": "kms:Decrypt",
        "Resource": "*"
      }
    ]
  }'
```

Replace `123456789012` with your account ID and `yourdata-worker-role` with the role your downstream SQS consumer uses.

### 5b. SQS access policy

Lock the queue so only the Lambda role can send and only the worker role can receive:

```bash
QUEUE_URL=$(aws sqs get-queue-url --queue-name yourdata-events-queue --output text --query QueueUrl)
QUEUE_ARN=$(aws sqs get-queue-attributes --queue-url "$QUEUE_URL" --attribute-names QueueArn --output text --query 'Attributes.QueueArn')

aws sqs set-queue-attributes \
  --queue-url "$QUEUE_URL" \
  --attributes '{
    "Policy": "{\"Version\":\"2012-10-17\",\"Statement\":[{\"Sid\":\"LambdaSend\",\"Effect\":\"Allow\",\"Principal\":{\"AWS\":\"arn:aws:iam::123456789012:role/yourdata-consent-ingress-role\"},\"Action\":\"sqs:SendMessage\",\"Resource\":\"'$QUEUE_ARN'\"},{\"Sid\":\"WorkerReceive\",\"Effect\":\"Allow\",\"Principal\":{\"AWS\":\"arn:aws:iam::123456789012:role/yourdata-worker-role\"},\"Action\":[\"sqs:ReceiveMessage\",\"sqs:DeleteMessage\",\"sqs:GetQueueAttributes\"],\"Resource\":\"'$QUEUE_ARN'\"},{\"Sid\":\"DenyAllOthers\",\"Effect\":\"Deny\",\"Principal\":\"*\",\"Action\":\"sqs:*\",\"Resource\":\"'$QUEUE_ARN'\",\"Condition\":{\"StringNotEquals\":{\"aws:PrincipalArn\":[\"arn:aws:iam::123456789012:role/yourdata-consent-ingress-role\",\"arn:aws:iam::123456789012:role/yourdata-worker-role\",\"arn:aws:iam::123456789012:root\"]}}}]}"
  }'
```

### 5c. DynamoDB table encryption

Enable encryption at rest with the project KMS key instead of the default AWS-owned key:

```bash
aws dynamodb update-table \
  --table-name yourdata-consents \
  --sse-specification Enabled=true,SSEType=KMS,KMSMasterKeyId=alias/yourdata-field-encryption \
  --region us-east-1
```

Then add `dynamodb:DescribeTable` and `kms:Decrypt` + `kms:GenerateDataKey` to the Lambda role if you enable this (the role's existing `kms:Encrypt` on the same key is not sufficient for DynamoDB SSE reads).

### 5d. Secrets Manager rotation

The deploy script creates a placeholder secret at `yourdata/ingress-credentials`. Replace it with real credentials and enable automatic rotation:

```bash
# Store actual credentials
aws secretsmanager put-secret-value \
  --secret-id yourdata/ingress-credentials \
  --secret-string '{"api_key":"<real-key>","db_password":"<real-password>"}' \
  --region us-east-1

# Enable rotation (requires a rotation Lambda — see AWS docs)
aws secretsmanager rotate-secret \
  --secret-id yourdata/ingress-credentials \
  --rotation-lambda-arn arn:aws:lambda:us-east-1:123456789012:function:yourdata-secret-rotator \
  --rotation-rules AutomaticallyAfterDays=30 \
  --region us-east-1
```

### 5e. API Gateway authorization

The deploy script creates the endpoint with `AUTHORIZATION_TYPE=NONE` for initial testing. Before production, add one of:

**Option 1: IAM authorization**

```bash
API_ID=<your-api-id>
RESOURCE_ID=<your-ingest-resource-id>

aws apigateway update-method \
  --rest-api-id "$API_ID" \
  --resource-id "$RESOURCE_ID" \
  --http-method POST \
  --patch-operations op=replace,path=/authorizationType,value=AWS_IAM

aws apigateway create-deployment --rest-api-id "$API_ID" --stage-name prod
```

Callers then sign requests with SigV4. Good for service-to-service.

**Option 2: Cognito user pool authorizer**

```bash
POOL_ARN=arn:aws:cognito-idp:us-east-1:123456789012:userpool/us-east-1_xxxxxxxxx

AUTHORIZER_ID=$(aws apigateway create-authorizer \
  --rest-api-id "$API_ID" \
  --name cognito-auth \
  --type COGNITO_USER_POOLS \
  --provider-arns "$POOL_ARN" \
  --identity-source method.request.header.Authorization \
  --output text --query id)

aws apigateway update-method \
  --rest-api-id "$API_ID" \
  --resource-id "$RESOURCE_ID" \
  --http-method POST \
  --patch-operations \
    op=replace,path=/authorizationType,value=COGNITO_USER_POOLS \
    op=replace,path=/authorizerId,value="$AUTHORIZER_ID"

aws apigateway create-deployment --rest-api-id "$API_ID" --stage-name prod
```

**Option 3: API key + usage plan**

```bash
aws apigateway create-api-key --name ingress-client-key --enabled --output text --query id
# then create a usage plan and associate the key + stage
```

### 5f. Enable API Gateway access logging

```bash
# Create a CloudWatch log group for API Gateway
aws logs create-log-group --log-group-name /aws/apigateway/yourdata-ingress-api

# Get the log group ARN
LOG_ARN=$(aws logs describe-log-groups \
  --log-group-name-prefix /aws/apigateway/yourdata-ingress-api \
  --query 'logGroups[0].arn' --output text)

# Enable on the stage
aws apigateway update-stage \
  --rest-api-id "$API_ID" \
  --stage-name prod \
  --patch-operations \
    op=replace,path=/accessLogSettings/destinationArn,value="$LOG_ARN" \
    'op=replace,path=/accessLogSettings/format,value={"requestId":"$context.requestId","ip":"$context.identity.sourceIp","caller":"$context.identity.caller","user":"$context.identity.user","requestTime":"$context.requestTime","httpMethod":"$context.httpMethod","resourcePath":"$context.resourcePath","status":"$context.status","protocol":"$context.protocol","responseLength":"$context.responseLength"}'
```

---

## 6. Complete local environment example

A minimal `.env.aws` file for your shell (source it before running deploy/tests):

```bash
# AWS authentication (Option A — access keys)
export AWS_ACCESS_KEY_ID=AKIA...............
export AWS_SECRET_ACCESS_KEY=wJalr...............
export AWS_REGION=us-east-1

# Resource prefix (must match --prefix passed to deploy-aws.sh)
export PREFIX=yourdata

# These are outputs from deploy-aws.sh — fill in after first deploy
export CONSENT_TABLE_NAME=yourdata-consents
export EVENT_QUEUE_URL=https://sqs.us-east-1.amazonaws.com/123456789012/yourdata-events-queue
export KMS_KEY_ID=alias/yourdata-field-encryption
```

Then:

```bash
source .env.aws

# Deploy
./scripts/deploy-aws.sh --region "$AWS_REGION" --prefix "$PREFIX"

# Run integration tests
cd lambda/consent-ingress
INTEGRATION=1 cargo test --test integration_tests

# Teardown
./scripts/teardown-aws.sh --region "$AWS_REGION" --prefix "$PREFIX"
```

---

## 7. Security checklist before production

- [ ] Replace `AdministratorAccess` on deployer with scoped policy (section 4a)
- [ ] Restrict KMS key policy to Lambda + worker roles only (section 5a)
- [ ] Apply SQS resource policy limiting send/receive principals (section 5b)
- [ ] Enable DynamoDB encryption at rest with project KMS key (section 5c)
- [ ] Rotate Secrets Manager placeholder with real credentials (section 5d)
- [ ] Add API Gateway authorization: IAM, Cognito, or API keys (section 5e)
- [ ] Enable API Gateway access logging (section 5f)
- [ ] Enable CloudTrail for the account/region
- [ ] Enable GuardDuty for anomaly detection
- [ ] Set a billing alarm for unexpected cost spikes
- [ ] Remove any long-lived access keys; prefer IAM Identity Center or instance roles
