CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    password TEXT NOT NULL,
    full_name TEXT,
    phone TEXT,
    api_token TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS consents (
    id UUID PRIMARY KEY,
    policy_version TEXT NOT NULL,
    privacy_notice_version TEXT NOT NULL,
    jurisdiction TEXT NOT NULL,
    marketing_opt_in BOOLEAN NOT NULL,
    analytics_opt_in BOOLEAN NOT NULL,
    personalization_opt_in BOOLEAN NOT NULL,
    privacy_notice_accepted BOOLEAN NOT NULL,
    terms_accepted BOOLEAN NOT NULL,
    data_processing_accepted BOOLEAN NOT NULL,
    source_ip TEXT,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS data_events (
    id UUID PRIMARY KEY,
    consent_id UUID NOT NULL REFERENCES consents(id),
    user_id UUID REFERENCES users(id),
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS form_submissions (
    id UUID PRIMARY KEY,
    consent_id UUID NOT NULL REFERENCES consents(id),
    user_id UUID REFERENCES users(id),
    form_name TEXT NOT NULL,
    fields JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS data_deletion_requests (
    id UUID PRIMARY KEY,
    consent_id UUID REFERENCES consents(id),
    user_id UUID REFERENCES users(id),
    email TEXT NOT NULL,
    jurisdiction TEXT NOT NULL,
    reason TEXT,
    status TEXT NOT NULL,
    requested_at TIMESTAMPTZ NOT NULL,
    processed_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS data_export_requests (
    id UUID PRIMARY KEY,
    consent_id UUID REFERENCES consents(id),
    user_id UUID REFERENCES users(id),
    email TEXT NOT NULL,
    jurisdiction TEXT NOT NULL,
    reason TEXT,
    status TEXT NOT NULL,
    requested_at TIMESTAMPTZ NOT NULL,
    processed_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS compliance_audit_logs (
    id UUID PRIMARY KEY,
    action TEXT NOT NULL,
    details JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS admin_sessions (
    id UUID PRIMARY KEY,
    token TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_data_events_consent_id ON data_events(consent_id);
CREATE INDEX IF NOT EXISTS idx_form_submissions_consent_id ON form_submissions(consent_id);
CREATE INDEX IF NOT EXISTS idx_consents_jurisdiction ON consents(jurisdiction);
CREATE INDEX IF NOT EXISTS idx_deletion_requests_status ON data_deletion_requests(status);
CREATE INDEX IF NOT EXISTS idx_export_requests_status ON data_export_requests(status);
CREATE INDEX IF NOT EXISTS idx_admin_sessions_expires_at ON admin_sessions(expires_at);
