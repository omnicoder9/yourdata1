#!/usr/bin/env bash
#
# teardown-aws.sh — Delete ALL AWS resources created by the consent-gated ingress pipeline.
#
# Usage:
#   ./scripts/teardown-aws.sh [--region us-east-1] [--prefix yourdata] [--yes]
#
# Flags:
#   --region   AWS region (default: us-east-1 or $AWS_REGION)
#   --prefix   Resource name prefix (default: yourdata)
#   --yes      Skip confirmation prompt
#
# This script deletes:
#   1. API Gateway REST API + deployments
#   2. Lambda function
#   3. CloudWatch log group
#   4. IAM inline policy + role
#   5. SQS queue
#   6. DynamoDB table
#   7. KMS key (schedules deletion with 7-day waiting period)
#   8. Secrets Manager secret (force-deleted, no recovery window)

set -euo pipefail

REGION="${AWS_REGION:-us-east-1}"
PREFIX="yourdata"
SKIP_CONFIRM=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --region) REGION="$2"; shift 2 ;;
        --prefix) PREFIX="$2"; shift 2 ;;
        --yes)    SKIP_CONFIRM=true; shift ;;
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

echo "=== Consent-Gated Ingress — AWS Teardown ==="
echo "Region:  ${REGION}"
echo "Prefix:  ${PREFIX}"
echo ""
echo "The following resources will be PERMANENTLY DELETED:"
echo "  - API Gateway:      ${API_NAME}"
echo "  - Lambda function:  ${LAMBDA_NAME}"
echo "  - CloudWatch logs:  ${LOG_GROUP}"
echo "  - IAM role+policy:  ${ROLE_NAME} / ${POLICY_NAME}"
echo "  - SQS queue:        ${QUEUE_NAME}"
echo "  - DynamoDB table:   ${TABLE_NAME}"
echo "  - KMS key:          ${KMS_ALIAS} (7-day scheduled deletion)"
echo "  - Secret:           ${SECRET_NAME}"
echo ""

if [[ "${SKIP_CONFIRM}" != "true" ]]; then
    read -rp "Type 'DELETE' to confirm: " CONFIRM
    if [[ "${CONFIRM}" != "DELETE" ]]; then
        echo "Aborted."
        exit 0
    fi
fi

echo ""

# Helper: run a command, swallow "not found" errors
safe() {
    "$@" 2>/dev/null || true
}

# -----------------------------------------------------------------------
# 1. API Gateway
# -----------------------------------------------------------------------
echo "[1/8] Deleting API Gateway: ${API_NAME}"
API_ID=$(aws apigateway get-rest-apis --region "${REGION}" \
    --query "items[?name=='${API_NAME}'].id" --output text 2>/dev/null || true)

if [[ -n "${API_ID}" && "${API_ID}" != "None" ]]; then
    safe aws apigateway delete-rest-api \
        --rest-api-id "${API_ID}" \
        --region "${REGION}"
    echo "  Deleted API: ${API_ID}"
else
    echo "  Not found, skipping."
fi
echo ""

# -----------------------------------------------------------------------
# 2. Lambda function
# -----------------------------------------------------------------------
echo "[2/8] Deleting Lambda function: ${LAMBDA_NAME}"
safe aws lambda delete-function \
    --function-name "${LAMBDA_NAME}" \
    --region "${REGION}"
echo "  Done."
echo ""

# -----------------------------------------------------------------------
# 3. CloudWatch log group
# -----------------------------------------------------------------------
echo "[3/8] Deleting CloudWatch log group: ${LOG_GROUP}"
safe aws logs delete-log-group \
    --log-group-name "${LOG_GROUP}" \
    --region "${REGION}"
echo "  Done."
echo ""

# -----------------------------------------------------------------------
# 4. IAM inline policy + role
# -----------------------------------------------------------------------
echo "[4/8] Deleting IAM role: ${ROLE_NAME}"
safe aws iam delete-role-policy \
    --role-name "${ROLE_NAME}" \
    --policy-name "${POLICY_NAME}"
safe aws iam delete-role \
    --role-name "${ROLE_NAME}"
echo "  Done."
echo ""

# -----------------------------------------------------------------------
# 5. SQS queue
# -----------------------------------------------------------------------
echo "[5/8] Deleting SQS queue: ${QUEUE_NAME}"
QUEUE_URL=$(aws sqs get-queue-url --queue-name "${QUEUE_NAME}" --region "${REGION}" \
    --output text --query 'QueueUrl' 2>/dev/null || true)

if [[ -n "${QUEUE_URL}" && "${QUEUE_URL}" != "None" ]]; then
    safe aws sqs delete-queue \
        --queue-url "${QUEUE_URL}" \
        --region "${REGION}"
    echo "  Deleted queue: ${QUEUE_URL}"
else
    echo "  Not found, skipping."
fi
echo ""

# -----------------------------------------------------------------------
# 6. DynamoDB table
# -----------------------------------------------------------------------
echo "[6/8] Deleting DynamoDB table: ${TABLE_NAME}"
if aws dynamodb describe-table --table-name "${TABLE_NAME}" --region "${REGION}" >/dev/null 2>&1; then
    aws dynamodb delete-table \
        --table-name "${TABLE_NAME}" \
        --region "${REGION}" \
        --output text --query 'TableDescription.TableStatus'
    echo "  Table deletion initiated."
else
    echo "  Not found, skipping."
fi
echo ""

# -----------------------------------------------------------------------
# 7. KMS key (schedule deletion — minimum 7 days)
# -----------------------------------------------------------------------
echo "[7/8] Scheduling KMS key deletion: ${KMS_ALIAS}"
KMS_KEY_ID=$(aws kms list-aliases --region "${REGION}" \
    --query "Aliases[?AliasName=='${KMS_ALIAS}'].TargetKeyId" --output text 2>/dev/null || true)

if [[ -n "${KMS_KEY_ID}" && "${KMS_KEY_ID}" != "None" ]]; then
    safe aws kms delete-alias \
        --alias-name "${KMS_ALIAS}" \
        --region "${REGION}"
    safe aws kms schedule-key-deletion \
        --key-id "${KMS_KEY_ID}" \
        --pending-window-in-days 7 \
        --region "${REGION}"
    echo "  Key ${KMS_KEY_ID} scheduled for deletion in 7 days."
    echo "  To cancel: aws kms cancel-key-deletion --key-id ${KMS_KEY_ID} --region ${REGION}"
else
    echo "  Not found, skipping."
fi
echo ""

# -----------------------------------------------------------------------
# 8. Secrets Manager
# -----------------------------------------------------------------------
echo "[8/8] Deleting Secrets Manager secret: ${SECRET_NAME}"
safe aws secretsmanager delete-secret \
    --secret-id "${SECRET_NAME}" \
    --force-delete-without-recovery \
    --region "${REGION}"
echo "  Done."
echo ""

# -----------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------
echo "=== Teardown Complete ==="
echo ""
echo "All resources have been deleted or scheduled for deletion."
echo ""
echo "Note: The KMS key has a 7-day waiting period before permanent deletion."
echo "      To cancel: aws kms cancel-key-deletion --key-id <key-id> --region ${REGION}"
