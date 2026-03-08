# Consent-Aware Event Ingestion Pipeline Epic (Agent Implementation Brief)

## Purpose of This Brief
This document defines how a coding agent should interpret and implement the epic end-to-end. It is intentionally detailed so execution quality can be evaluated against specific technical and product outcomes, not just high-level architecture statements.

## Epic
**Consent-Aware Event Ingestion Pipeline**

Build a production-grade ingestion pipeline that accepts frontend telemetry/events only when consent is valid, processes events asynchronously, persists them to operational and analytical stores, and enforces security controls for sensitive data.

Core stack constraints:
- Ingress/API: `API Gateway` + `Lambda`
- Queue: `SQS`
- Processing: Rust workers
- Storage: `DynamoDB` (hot path), `S3` (archive)
- Analytics stream: `Firehose`
- Security: `KMS` encryption for sensitive payload fields, `Secrets Manager` for credentials/secrets

---

## Opening Prompt Intent (What the Agent Should Understand)
The prompt is not asking for a toy diagram or a single function. It expects:
- A full implementation plan and (where requested) code/config changes spanning infra, API contracts, worker logic, observability, and security.
- Strict consent enforcement at ingress and re-validation in processing (defense in depth).
- Operationally sound behavior: retries, idempotency, dead-letter handling, metrics, and traceability.
- Data lifecycle handling: fast query path, immutable archive, and analytics fanout.

A correct model response should include:
- Concrete architecture decisions and rationale.
- Message schemas and validation rules.
- Error handling semantics and retry strategy.
- Security boundary details and key management behavior.
- Explicit rollout/test plan.

---

## Ideal End State After Implementation
When complete, system behavior should change in these high-level ways:
- Frontend events no longer directly hit the monolith DB endpoint; they enter through API Gateway/Lambda.
- Events without valid consent are rejected early with auditable reason codes.
- Accepted events are durably queued and processed asynchronously.
- Operational reads use DynamoDB (low-latency recent data).
- Compliance/forensics can retrieve immutable event copies from S3.
- Analytics teams consume normalized streams via Firehose destination.
- Sensitive fields are encrypted at field-level with KMS before persistence/egress.
- Secrets are never hard-coded or committed; runtime fetch is via Secrets Manager.

Success indicators:
- At-least-once ingestion with idempotent processing.
- End-to-end traceability from request ID -> queue message -> storage writes.
- No plaintext sensitive fields in logs or long-term stores unless explicitly allowed.

---

## Feature Breakdown

### Feature 1: Consent-Gated Ingress
- API Gateway endpoint for event ingestion.
- Lambda validates schema + required consent metadata.
- Lambda checks consent status (source of truth service/table).
- If valid: enqueue to SQS with correlation IDs and idempotency key.
- If invalid: 4xx response with machine-readable rejection reason.

### Feature 2: Asynchronous Rust Processing
- Rust worker polls SQS, validates/normalizes message.
- Upserts event into DynamoDB (hot path).
- Writes full event envelope to S3 archive partitioned by date/jurisdiction.
- Pushes normalized analytics payload to Firehose.
- Uses partial batch failure semantics so only failed messages are retried.

### Feature 3: Security & Secrets
- KMS encryption for selected payload attributes (PII/sensitive fields).
- Secrets Manager for API keys/DB credentials/any tokenized secrets.
- IAM least privilege for Lambda, worker runtime, and Firehose/S3 writers.

### Feature 4: Reliability & Observability
- DLQ for poison messages.
- Structured logs with PII-safe redaction.
- Metrics: accepted/rejected count, queue lag, processing latency, failure rate.
- Alarms for DLQ depth and ingestion error spikes.

---

## User Stories
1. **As a user**, I want my event data rejected if consent is missing or revoked so data collection respects my preferences.
2. **As a compliance lead**, I want every rejected/accepted ingestion decision auditable so we can prove lawful basis.
3. **As a platform engineer**, I want event ingestion decoupled with SQS so traffic spikes do not drop events.
4. **As an analytics engineer**, I want normalized streams in Firehose and durable raw archives in S3 so I can run both real-time and historical analysis.
5. **As a security engineer**, I want sensitive payload fields encrypted with KMS and secrets managed centrally so accidental exposure risk is reduced.

---

## User Acceptance Criteria (UAC)

### Ingress & Consent
- `Given` a valid consent ID and valid payload, `when` event is submitted, `then` API returns success and message is enqueued.
- `Given` missing/invalid/revoked consent, `when` event is submitted, `then` API returns `4xx` with deterministic error code and event is not enqueued.
- `Given` malformed payload, `when` submitted, `then` API returns schema validation error and logs include correlation ID.

### Processing
- `Given` queued valid event, `when` worker processes it, `then` record appears in DynamoDB and S3 archive and analytics payload is delivered via Firehose.
- `Given` transient downstream failure, `when` processing retries, `then` no duplicate durable writes occur (idempotent behavior).
- `Given` repeated permanent failure, `when retry limit exceeded`, `then` message lands in DLQ with failure context.

### Security
- Sensitive fields are encrypted with KMS before persistence/forwarding.
- No credentials are present in code, `.env.example` may only reference secret names/ARNs.
- IAM policies block unauthorized actions (validated by negative tests).

### Observability
- Dashboards/metrics exist for ingestion throughput, rejection rates, worker failures, and DLQ backlog.
- Logs are structured JSON and redact sensitive fields.

---

## What a Correct Model Output Should Include
The model should produce **implementation-ready** artifacts, not just prose:
- API contract (request/response schema + error model).
- Queue message envelope spec (event metadata, consent metadata, idempotency key, trace IDs).
- Encryption policy matrix (which fields encrypted/hash/tokenized).
- Infrastructure definitions (Terraform/CDK/CloudFormation or equivalent).
- Worker flow with retry/idempotency/DLQ semantics.
- Test matrix (unit, integration, failure injection, compliance/security checks).
- Rollout plan with backfill/migration notes and fallback strategy.

---

## Expected Repo Changes (Concrete)
Adjust paths to your repository layout, but a complete implementation usually touches these areas.

### Add
- `infra/` (or `terraform/`/`cdk/`) for:
  - API Gateway resources
  - Lambda function + IAM role
  - SQS queue + DLQ
  - DynamoDB table(s)
  - S3 bucket/prefix policies
  - Firehose delivery stream
  - KMS key/aliases/policies
  - Secrets Manager secret definitions/references
- `backend/lambda/ingest_handler.rs` (or language-appropriate Lambda handler)
- `backend/worker/src/sqs_consumer.rs`
- `backend/worker/src/persistence/dynamo.rs`
- `backend/worker/src/persistence/s3_archive.rs`
- `backend/worker/src/stream/firehose.rs`
- `backend/worker/src/security/encryption.rs`
- `backend/worker/src/models/event_envelope.rs`
- `docs/consent-ingestion-runbook.md`
- `docs/data-classification-and-encryption.md`

### Modify
- Frontend event submission client to call API Gateway endpoint and include consent metadata.
- Existing backend ingestion endpoints to either:
  - become internal-only, or
  - proxy/compat mode during migration.
- Existing consent service/table access layer for low-latency consent checks.
- CI/CD pipeline to deploy infra + Lambda + worker and run integration tests.
- `.gitignore` if generated local infra state/build artifacts are introduced.

### Remove/Deprecate
- Direct synchronous write path from frontend to DB for event ingestion (after migration window).

---

## Suggested Function-Level Expectations (Example)
If Rust is used for worker/lambda:
- `validate_event_schema(...) -> Result<ValidatedEvent, ValidationError>`
- `validate_consent(...) -> Result<ConsentState, ConsentError>`
- `build_envelope(...) -> EventEnvelope`
- `encrypt_sensitive_fields(...) -> EventEnvelope`
- `persist_hot_path(...)` (DynamoDB)
- `archive_raw_event(...)` (S3)
- `emit_analytics_record(...)` (Firehose)
- `handle_batch_partial_failures(...)` (SQS partial batch response)
- `idempotency_guard(...)` to prevent duplicate writes

---

## If Prompt Is Review-Only (No Code Changes)
Expected findings/suggestions should include:
- Missing idempotency keys or dedupe design.
- Consent check performed only once (should be defense in depth).
- No clear revoked-consent handling for already queued messages.
- KMS applied only at rest, not field-level where required.
- Missing DLQ alarm thresholds/runbook.
- Overly broad IAM policies.
- Potential PII leakage in logs/traces.
- Firehose schema drift risk without versioning.

---

## Common Failure Modes (What Agents Often Miss)
1. **Consent race conditions**: consent valid at enqueue but revoked before consume; worker must re-check policy.
2. **Duplicate processing**: retries causing duplicate S3/Dynamo/Firehose writes without idempotency.
3. **Partial failure handling**: entire SQS batch retried when only one message failed.
4. **Encryption gaps**: encrypting full blob but still logging plaintext fields.
5. **Schema evolution**: no event versioning leading to broken downstream analytics.
6. **Secrets sprawl**: embedding secrets in env/config files instead of Secrets Manager.
7. **Insufficient partitioning**: poor S3 key design harming query performance and retention management.
8. **No backpressure plan**: queue depth grows without autoscaling worker consumers.

---

## Non-Obvious Quality Bar
A high-quality implementation should also include:
- Contract tests between API Gateway/Lambda and worker message format.
- Replay tooling for DLQ messages.
- Explicit retention/deletion alignment across DynamoDB + S3 + analytics sink.
- Event lineage fields (`event_id`, `consent_id`, `trace_id`, `ingest_ts`, `schema_version`, `jurisdiction`).
- Operational runbook with on-call actions for elevated rejection rates and Firehose delivery failures.

---

## Definition of Done (DoD)
- All UAC scenarios pass in automated tests.
- Infrastructure is deployable in non-prod with least-privilege IAM.
- Security review confirms no plaintext sensitive fields outside approved boundaries.
- Observability and alerting dashboards exist and are documented.
- Migration path from legacy ingestion path is executed and verified.
