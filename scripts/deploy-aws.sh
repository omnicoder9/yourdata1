#!/usr/bin/env bash
#
# deploy-aws.sh — Provision all AWS resources for the consent-gated ingress pipeline.
#
# Usage:
#   ./scripts/deploy-aws.sh [--region us-east-1] [--prefix yourdata]
#
# Prerequisites:
#   - AWS CLI v2 configured with appropriate credentials
#   - cargo-lambda (auto-installed into a venv if missing)
#   - Python 3 with venv support (for cargo-lambda/zig installation)
#
# This script creates:
#   1. DynamoDB table for consent records
#   2. SQS queue for accepted events
#   3. KMS key for sensitive-field encryption
#   4. IAM role + policy for the Lambda function
#   5. Lambda function (built with cargo-lambda)
#   6. API Gateway REST API wired to the Lambda
#   7. Secrets Manager secret (placeholder for credentials)

set -euo pipefail

REGION="${AWS_REGION:-us-east-1}"
PREFIX="yourdata"
ACCOUNT_ID=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --region) REGION="$2"; shift 2 ;;
        --prefix) PREFIX="$2"; shift 2 ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

TABLE_NAME="${PREFIX}-consents"
QUEUE_NAME="${PREFIX}-events-queue"
KMS_ALIAS="alias/${PREFIX}-field-encryption"
LAMBDA_NAME="${PREFIX}-consent-ingress"
API_NAME="${PREFIX}-ingress-api"
ROLE_NAME="${PREFIX}-consent-ingress-role"
POLICY_NAME="${PREFIX}-consent-ingress-policy"
SECRET_NAME="${PREFIX}/ingress-credentials"
LOG_GROUP="/aws/lambda/${LAMBDA_NAME}"

# -----------------------------------------------------------------------
# 0. Ensure cargo-lambda and zig are available
# -----------------------------------------------------------------------
ensure_cargo_lambda() {
    if cargo lambda --version >/dev/null 2>&1; then
        return 0
    fi

    VENV_DIR="${HOME}/.local/share/cargo-lambda-venv"
    if [[ -x "${VENV_DIR}/bin/cargo-lambda" ]]; then
        mkdir -p "${HOME}/.local/bin"
        ln -sf "${VENV_DIR}/bin/cargo-lambda" "${HOME}/.local/bin/cargo-lambda"
        # ziglang installs as python-zig in the venv
        if [[ -x "${VENV_DIR}/bin/python-zig" ]]; then
            ln -sf "${VENV_DIR}/bin/python-zig" "${HOME}/.local/bin/zig"
        fi
        export PATH="${HOME}/.local/bin:${PATH}"
        if cargo lambda --version >/dev/null 2>&1; then
            return 0
        fi
    fi

    echo "cargo-lambda not found — installing via Python venv..."
    python3 -m venv "${VENV_DIR}"
    "${VENV_DIR}/bin/pip" install --quiet cargo-lambda
    mkdir -p "${HOME}/.local/bin"
    ln -sf "${VENV_DIR}/bin/cargo-lambda" "${HOME}/.local/bin/cargo-lambda"
    if [[ -x "${VENV_DIR}/bin/python-zig" ]]; then
        ln -sf "${VENV_DIR}/bin/python-zig" "${HOME}/.local/bin/zig"
    fi
    export PATH="${HOME}/.local/bin:${PATH}"
    echo "  Installed: $(cargo lambda --version)"
}

ensure_cargo_lambda

echo "=== Consent-Gated Ingress — AWS Deploy ==="
echo "Region:  ${REGION}"
echo "Prefix:  ${PREFIX}"
echo ""

ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text --region "${REGION}")
echo "Account: ${ACCOUNT_ID}"
echo ""

# -----------------------------------------------------------------------
# 1. DynamoDB table
# -----------------------------------------------------------------------
echo "[1/7] Creating DynamoDB table: ${TABLE_NAME}"
if aws dynamodb describe-table --table-name "${TABLE_NAME}" --region "${REGION}" >/dev/null 2>&1; then
    echo "  Table already exists, skipping."
else
    aws dynamodb create-table \
        --table-name "${TABLE_NAME}" \
        --attribute-definitions AttributeName=consent_id,AttributeType=S \
        --key-schema AttributeName=consent_id,KeyType=HASH \
        --billing-mode PAY_PER_REQUEST \
        --region "${REGION}" \
        --output text --query 'TableDescription.TableArn'
    echo "  Waiting for table to become active..."
    aws dynamodb wait table-exists --table-name "${TABLE_NAME}" --region "${REGION}"
    echo "  Done."
fi
echo ""

# -----------------------------------------------------------------------
# 2. SQS queue
# -----------------------------------------------------------------------
echo "[2/7] Creating SQS queue: ${QUEUE_NAME}"
QUEUE_URL=$(aws sqs create-queue \
    --queue-name "${QUEUE_NAME}" \
    --attributes '{"VisibilityTimeout":"60","MessageRetentionPeriod":"1209600"}' \
    --region "${REGION}" \
    --output text --query 'QueueUrl')
echo "  Queue URL: ${QUEUE_URL}"

QUEUE_ARN=$(aws sqs get-queue-attributes \
    --queue-url "${QUEUE_URL}" \
    --attribute-names QueueArn \
    --region "${REGION}" \
    --output text --query 'Attributes.QueueArn')
echo ""

# -----------------------------------------------------------------------
# 3. KMS key
# -----------------------------------------------------------------------
echo "[3/7] Creating KMS key: ${KMS_ALIAS}"
EXISTING_KEY=$(aws kms list-aliases --region "${REGION}" \
    --query "Aliases[?AliasName=='${KMS_ALIAS}'].TargetKeyId" --output text 2>/dev/null || true)

if [[ -n "${EXISTING_KEY}" && "${EXISTING_KEY}" != "None" ]]; then
    KMS_KEY_ID="${EXISTING_KEY}"
    echo "  Key already exists: ${KMS_KEY_ID}"
else
    KMS_KEY_ID=$(aws kms create-key \
        --description "Field-level encryption for ${PREFIX} ingress pipeline" \
        --region "${REGION}" \
        --output text --query 'KeyMetadata.KeyId')
    aws kms create-alias \
        --alias-name "${KMS_ALIAS}" \
        --target-key-id "${KMS_KEY_ID}" \
        --region "${REGION}"
    echo "  Key created: ${KMS_KEY_ID}"
fi
echo ""

# -----------------------------------------------------------------------
# 4. IAM role + policy
# -----------------------------------------------------------------------
echo "[4/7] Creating IAM role: ${ROLE_NAME}"
TRUST_POLICY=$(cat <<'TRUST'
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": { "Service": "lambda.amazonaws.com" },
      "Action": "sts:AssumeRole"
    }
  ]
}
TRUST
)

ROLE_ARN=$(aws iam get-role --role-name "${ROLE_NAME}" \
    --query 'Role.Arn' --output text 2>/dev/null || true)

if [[ -z "${ROLE_ARN}" || "${ROLE_ARN}" == "None" ]]; then
    ROLE_ARN=$(aws iam create-role \
        --role-name "${ROLE_NAME}" \
        --assume-role-policy-document "${TRUST_POLICY}" \
        --output text --query 'Role.Arn')
    echo "  Role created: ${ROLE_ARN}"
    # Allow role propagation
    sleep 10
else
    echo "  Role exists: ${ROLE_ARN}"
fi

TABLE_ARN="arn:aws:dynamodb:${REGION}:${ACCOUNT_ID}:table/${TABLE_NAME}"
KMS_ARN="arn:aws:kms:${REGION}:${ACCOUNT_ID}:key/${KMS_KEY_ID}"

INLINE_POLICY=$(cat <<EOF
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "DynamoDBRead",
      "Effect": "Allow",
      "Action": ["dynamodb:GetItem"],
      "Resource": "${TABLE_ARN}"
    },
    {
      "Sid": "SQSSend",
      "Effect": "Allow",
      "Action": ["sqs:SendMessage"],
      "Resource": "${QUEUE_ARN}"
    },
    {
      "Sid": "KMSEncrypt",
      "Effect": "Allow",
      "Action": ["kms:Encrypt"],
      "Resource": "${KMS_ARN}"
    },
    {
      "Sid": "CloudWatchLogs",
      "Effect": "Allow",
      "Action": [
        "logs:CreateLogGroup",
        "logs:CreateLogStream",
        "logs:PutLogEvents"
      ],
      "Resource": "arn:aws:logs:${REGION}:${ACCOUNT_ID}:log-group:${LOG_GROUP}:*"
    }
  ]
}
EOF
)

aws iam put-role-policy \
    --role-name "${ROLE_NAME}" \
    --policy-name "${POLICY_NAME}" \
    --policy-document "${INLINE_POLICY}"
echo "  Inline policy attached."
echo ""

# -----------------------------------------------------------------------
# 5. Build + deploy Lambda
# -----------------------------------------------------------------------
echo "[5/7] Building Lambda function with cargo-lambda"
LAMBDA_DIR="$(cd "$(dirname "$0")/../lambda/consent-ingress" && pwd)"

(cd "${LAMBDA_DIR}" && cargo lambda build --release --output-format zip)

ZIP_PATH="${LAMBDA_DIR}/target/lambda/consent-ingress/bootstrap.zip"

LAMBDA_ENV="Variables={CONSENT_TABLE_NAME=${TABLE_NAME},EVENT_QUEUE_URL=${QUEUE_URL},KMS_KEY_ID=${KMS_KEY_ID},RUST_LOG=info}"

echo "  Deploying Lambda: ${LAMBDA_NAME}"
EXISTING_LAMBDA=$(aws lambda get-function --function-name "${LAMBDA_NAME}" \
    --region "${REGION}" --query 'Configuration.FunctionArn' --output text 2>/dev/null || true)

if [[ -n "${EXISTING_LAMBDA}" && "${EXISTING_LAMBDA}" != "None" ]]; then
    # Wait for any in-progress update to finish before applying ours
    aws lambda wait function-updated \
        --function-name "${LAMBDA_NAME}" \
        --region "${REGION}" 2>/dev/null || true

    aws lambda update-function-code \
        --function-name "${LAMBDA_NAME}" \
        --zip-file "fileb://${ZIP_PATH}" \
        --region "${REGION}" \
        --output text --query 'FunctionArn'
    echo "  Lambda code updated."

    # Wait for code update to settle before updating configuration
    aws lambda wait function-updated \
        --function-name "${LAMBDA_NAME}" \
        --region "${REGION}"

    aws lambda update-function-configuration \
        --function-name "${LAMBDA_NAME}" \
        --role "${ROLE_ARN}" \
        --timeout 30 \
        --memory-size 256 \
        --environment "${LAMBDA_ENV}" \
        --region "${REGION}" \
        --output text --query 'FunctionArn' >/dev/null
    echo "  Lambda configuration updated."
else
    aws lambda create-function \
        --function-name "${LAMBDA_NAME}" \
        --runtime provided.al2023 \
        --handler bootstrap \
        --role "${ROLE_ARN}" \
        --zip-file "fileb://${ZIP_PATH}" \
        --timeout 30 \
        --memory-size 256 \
        --environment "${LAMBDA_ENV}" \
        --region "${REGION}" \
        --output text --query 'FunctionArn'
    echo "  Lambda created."

    # Wait for function to become active before API Gateway wiring
    aws lambda wait function-active-v2 \
        --function-name "${LAMBDA_NAME}" \
        --region "${REGION}"
fi
echo ""

# -----------------------------------------------------------------------
# 6. API Gateway
# -----------------------------------------------------------------------
echo "[6/7] Creating API Gateway: ${API_NAME}"
EXISTING_API=$(aws apigateway get-rest-apis --region "${REGION}" \
    --query "items[?name=='${API_NAME}'].id" --output text 2>/dev/null || true)

if [[ -n "${EXISTING_API}" && "${EXISTING_API}" != "None" ]]; then
    API_ID="${EXISTING_API}"
    echo "  API already exists: ${API_ID}"
else
    API_ID=$(aws apigateway create-rest-api \
        --name "${API_NAME}" \
        --description "Consent-gated event ingress" \
        --endpoint-configuration types=REGIONAL \
        --region "${REGION}" \
        --output text --query 'id')
    echo "  API created: ${API_ID}"
fi

ROOT_ID=$(aws apigateway get-resources --rest-api-id "${API_ID}" --region "${REGION}" \
    --query 'items[?path==`/`].id' --output text)

# Create /ingest resource
INGEST_ID=$(aws apigateway get-resources --rest-api-id "${API_ID}" --region "${REGION}" \
    --query "items[?pathPart=='ingest'].id" --output text 2>/dev/null || true)

if [[ -z "${INGEST_ID}" || "${INGEST_ID}" == "None" ]]; then
    INGEST_ID=$(aws apigateway create-resource \
        --rest-api-id "${API_ID}" \
        --parent-id "${ROOT_ID}" \
        --path-part "ingest" \
        --region "${REGION}" \
        --output text --query 'id')
fi

# POST method
aws apigateway put-method \
    --rest-api-id "${API_ID}" \
    --resource-id "${INGEST_ID}" \
    --http-method POST \
    --authorization-type NONE \
    --region "${REGION}" >/dev/null 2>&1 || true

LAMBDA_ARN="arn:aws:lambda:${REGION}:${ACCOUNT_ID}:function:${LAMBDA_NAME}"
LAMBDA_URI="arn:aws:apigateway:${REGION}:lambda:path/2015-03-31/functions/${LAMBDA_ARN}/invocations"

aws apigateway put-integration \
    --rest-api-id "${API_ID}" \
    --resource-id "${INGEST_ID}" \
    --http-method POST \
    --type AWS_PROXY \
    --integration-http-method POST \
    --uri "${LAMBDA_URI}" \
    --region "${REGION}" >/dev/null

# Grant API Gateway permission to invoke Lambda
aws lambda add-permission \
    --function-name "${LAMBDA_NAME}" \
    --statement-id "apigateway-invoke-${API_ID}" \
    --action "lambda:InvokeFunction" \
    --principal apigateway.amazonaws.com \
    --source-arn "arn:aws:execute-api:${REGION}:${ACCOUNT_ID}:${API_ID}/*/POST/ingest" \
    --region "${REGION}" 2>/dev/null || true

# Deploy to stage (create-deployment is always safe — it snapshots current config)
DEPLOY_ID=$(aws apigateway create-deployment \
    --rest-api-id "${API_ID}" \
    --stage-name prod \
    --description "deploy-aws.sh $(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --region "${REGION}" \
    --output text --query 'id')

INVOKE_URL="https://${API_ID}.execute-api.${REGION}.amazonaws.com/prod/ingest"
echo "  Deployment: ${DEPLOY_ID}"
echo "  Endpoint: ${INVOKE_URL}"
echo ""

# -----------------------------------------------------------------------
# 7. Secrets Manager (placeholder)
# -----------------------------------------------------------------------
echo "[7/7] Creating Secrets Manager secret: ${SECRET_NAME}"
EXISTING_SECRET=$(aws secretsmanager describe-secret --secret-id "${SECRET_NAME}" \
    --region "${REGION}" --query 'ARN' --output text 2>/dev/null || true)

if [[ -n "${EXISTING_SECRET}" && "${EXISTING_SECRET}" != "None" ]]; then
    echo "  Secret already exists."
else
    aws secretsmanager create-secret \
        --name "${SECRET_NAME}" \
        --description "Credentials for ${PREFIX} ingress pipeline" \
        --secret-string '{"placeholder":"replace-with-real-credentials"}' \
        --region "${REGION}" \
        --output text --query 'ARN'
    echo "  Secret created."
fi
echo ""

# -----------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------
echo "=== Deploy Complete ==="
echo ""
echo "Resources created:"
echo "  DynamoDB table:   ${TABLE_NAME}"
echo "  SQS queue:        ${QUEUE_URL}"
echo "  KMS key:          ${KMS_KEY_ID} (${KMS_ALIAS})"
echo "  Lambda function:  ${LAMBDA_NAME}"
echo "  API Gateway:      ${INVOKE_URL}"
echo "  IAM role:         ${ROLE_NAME}"
echo "  Secrets Manager:  ${SECRET_NAME}"
echo ""
cat <<USAGE
Seed a consent record, then test:

  CONSENT_ID="\$(python3 -c 'import uuid; print(uuid.uuid4())')"

  aws dynamodb put-item --table-name ${TABLE_NAME} --region ${REGION} --item '{
    "consent_id":{"S":"'"\\\$CONSENT_ID"'"},
    "status":{"S":"active"},
    "jurisdiction":{"S":"EU"},
    "policy_version":{"S":"v1.0"},
    "analytics_opt_in":{"BOOL":true},
    "marketing_opt_in":{"BOOL":false},
    "personalization_opt_in":{"BOOL":true},
    "data_processing_accepted":{"BOOL":true},
    "created_at":{"S":"2025-06-01T00:00:00Z"}
  }'

  curl -X POST ${INVOKE_URL} \\
    -H 'Content-Type: application/json' \\
    -d '{"consent_id":"'"\\\$CONSENT_ID"'","event_type":"page_view","payload":{},"jurisdiction":"EU","policy_version":"v1.0","purpose":"analytics"}'

To tear down all resources:
  ./scripts/teardown-aws.sh --region ${REGION} --prefix ${PREFIX}
USAGE
