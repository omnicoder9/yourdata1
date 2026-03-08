# Consent-Based Data Collector (Test)

This repo contains a full-stack app designed to collect as much consented user data as possible.

## Stack

- Frontend: Vue 3 + TypeScript + Vite
- Backend: Rust + Rocket + sqlx
- Database: PostgreSQL (test only)

## Project layout

- `frontend/`: consent + optional signup + data forms UI
- `backend/`: API service and DB access
- `backend/sql/init.sql`: test schema
- `docker-compose.yml`: local PostgreSQL test instance
- `DOCKER_TROUBLESHOOTING_ARCH.md`: Docker daemon/module troubleshooting on Arch Linux
- `LEGAL_COMPLIANCE.md`: legal text, retention/deletion workflows, and jurisdiction controls

## Data collection design

- No event/form ingestion is accepted unless a valid `consent_id` exists.
- Consent stores policy version, privacy notice version, jurisdiction, and per-purpose opt-ins.
- Optional account creation stores email/password/full name/phone.
- Automatic telemetry is submitted after consent (UA, platform, timezone, screen, referrer, URL).
- Additional structured profile data is submitted through a form endpoint.
- User-facing export/deletion request workflows are available and tracked.
- Admin-only processing endpoints are protected by `X-Admin-Key`.
- A separate explainer page is available at `/data-use-explainer` with risks, use cases, and authoritative references.
- A separate admin portal is available at `/admin` to view DB tables through authenticated API sessions.

## Local setup (without Docker)

1. Install/start PostgreSQL:

```bash
sudo pacman -S postgresql
sudo -iu postgres initdb -D /var/lib/postgres/data
sudo systemctl enable --now postgresql
```

2. Create test DB/user:

```bash
sudo -iu postgres psql -c "CREATE USER collector WITH PASSWORD 'collector';"
sudo -iu postgres psql -c "CREATE DATABASE collector_test OWNER collector;"
```

3. Apply schema:

```bash
psql "postgres://collector:collector@localhost:5432/collector_test" -f backend/sql/init.sql
```

4. Run backend:

```bash
cd backend
cp .env.example .env
cargo run
```

5. Run frontend:

```bash
cd frontend
cp .env.example .env
npm install
npm run dev
```

## Local setup (with Docker, optional)

1. Start PostgreSQL test DB:

```bash
docker compose up -d postgres
```

2. Run backend:

```bash
cd backend
cp .env.example .env
cargo run
```

3. Run frontend:

```bash
cd frontend
cp .env.example .env
npm install
npm run dev
```

## API endpoints

- `GET /api/health`
- `GET /api/compliance/legal-text`
- `POST /api/consents`
- `POST /api/users/register`
- `POST /api/data/events` (requires valid `consent_id`)
- `POST /api/data/forms` (requires valid `consent_id`)
- `POST /api/compliance/deletion-requests`
- `POST /api/compliance/export-requests`
- `POST /api/compliance/deletion-requests/<request_id>/process` (requires `X-Admin-Key`)
- `POST /api/compliance/export-requests/<request_id>/process` (requires `X-Admin-Key`)
- `POST /api/compliance/retention/run` (requires `X-Admin-Key`)
- `POST /api/admin/login`
- `GET /api/admin/tables` (requires `Authorization: Bearer <session_token>`)
- `GET /api/admin/tables/<table>?limit=<n>` (requires `Authorization: Bearer <session_token>`)

## Admin configuration

Set `ADMIN_API_KEY` in `backend/.env` for compliance processing endpoints:

```bash
ADMIN_API_KEY=change-me
```

`/api/admin/login` validates this key and creates an expiring admin session token used by `/admin` portal requests.
The admin table endpoint intentionally excludes the `users.password` column.

## Schema update note

If your local DB was created before newer tables/columns were added, re-apply schema:

```bash
psql "postgres://collector:collector@localhost:5432/collector_test" -f backend/sql/init.sql
```

## Important notes

- This implementation is for testing/prototyping only.
- Passwords are currently stored in plain text and must be replaced with proper password hashing before any real deployment.
- Implement legal review, identity verification for rights requests, and production-grade audit/reporting before deployment.
