# Legal Text, Retention, and Compliance Controls

## Privacy Notice (v1.0.0)

This test service collects personal data only after explicit consent. Data categories can include contact details, account identifiers, device/browser metadata, submitted profile fields, and in-app interaction events. Collection purposes are limited to analytics, personalization, and communications according to each user's selected opt-in preferences.

## Terms of Use (v1.0.0)

By using this portal, users confirm they are authorized to provide submitted data and agree to processing under the selected consent options. Users can request data export or deletion using the provided rights workflows.

## Jurisdiction-Specific Controls

- `EU`: retention target 180 days
- `UK`: retention target 365 days
- `US-CA`: retention target 365 days
- `US-OTHER`: retention target 540 days
- `OTHER`: retention target 365 days

Jurisdiction is captured at consent time and used by retention jobs.

## Retention Workflow

Admin endpoint:

- `POST /api/compliance/retention/run`
- Requires header: `X-Admin-Key: <ADMIN_API_KEY>`

Behavior:

- Deletes expired rows from `data_events` and `form_submissions` based on consent-linked jurisdiction.
- Writes an audit record to `compliance_audit_logs`.

## Deletion Workflow

User-facing request endpoint:

- `POST /api/compliance/deletion-requests`

Admin processing endpoint:

- `POST /api/compliance/deletion-requests/<request_id>/process`
- Requires header: `X-Admin-Key: <ADMIN_API_KEY>`

Behavior:

- Deletes user-linked and/or consent-linked data from `data_events` and `form_submissions`.
- Deletes user account row when `user_id` is provided.
- Marks request as completed and writes an audit log.

## Export Workflow

User-facing request endpoint:

- `POST /api/compliance/export-requests`

Admin processing endpoint:

- `POST /api/compliance/export-requests/<request_id>/process`
- Requires header: `X-Admin-Key: <ADMIN_API_KEY>`

Behavior:

- Returns exported user/events/forms payload in JSON.
- Marks request as completed and writes an audit log.

## Operational Notes

- Current implementation is test/prototype oriented.
- Replace plain-text passwords with secure password hashing before production.
- Add identity verification steps and service-level timelines for rights requests before production use.
