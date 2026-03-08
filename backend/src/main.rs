#[macro_use]
extern crate rocket;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use rand::RngCore;
use rocket::fairing::{self, Fairing};
use rocket::form::FromForm;
use rocket::http::{Header, Status};
use rocket::request::{FromRequest, Outcome};
use rocket::serde::{json::Json, Deserialize, Serialize};
use rocket::{Build, Request, Response, Rocket, State};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    db: PgPool,
    admin_api_key: Option<String>,
}

struct AdminKey;
struct AdminSession;

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AdminKey {
    type Error = Json<ApiError>;

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let state = match request.rocket().state::<AppState>() {
            Some(s) => s,
            None => {
                return Outcome::Error((
                    Status::InternalServerError,
                    Json(ApiError {
                        error: "Missing app state".to_string(),
                    }),
                ));
            }
        };

        let expected = match &state.admin_api_key {
            Some(v) if !v.is_empty() => v,
            _ => {
                return Outcome::Error((
                    Status::Forbidden,
                    Json(ApiError {
                        error: "ADMIN_API_KEY is not configured".to_string(),
                    }),
                ));
            }
        };

        match request.headers().get_one("X-Admin-Key") {
            Some(provided) if provided == expected => Outcome::Success(AdminKey),
            _ => Outcome::Error((
                Status::Unauthorized,
                Json(ApiError {
                    error: "Invalid admin key".to_string(),
                }),
            )),
        }
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AdminSession {
    type Error = Json<ApiError>;

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let state = match request.rocket().state::<AppState>() {
            Some(s) => s,
            None => {
                return Outcome::Error((
                    Status::InternalServerError,
                    Json(ApiError {
                        error: "Missing app state".to_string(),
                    }),
                ));
            }
        };

        let auth = match request.headers().get_one("Authorization") {
            Some(v) => v,
            None => {
                return Outcome::Error((
                    Status::Unauthorized,
                    Json(ApiError {
                        error: "Missing Authorization header".to_string(),
                    }),
                ));
            }
        };

        let token = match auth.strip_prefix("Bearer ") {
            Some(v) if !v.is_empty() => v,
            _ => {
                return Outcome::Error((
                    Status::Unauthorized,
                    Json(ApiError {
                        error: "Authorization must be a Bearer token".to_string(),
                    }),
                ));
            }
        };

        let valid = sqlx::query(
            r#"
            SELECT token
            FROM admin_sessions
            WHERE token = $1
              AND expires_at > NOW()
            "#,
        )
        .bind(token)
        .fetch_optional(&state.db)
        .await;

        match valid {
            Ok(Some(_)) => Outcome::Success(AdminSession),
            Ok(None) => Outcome::Error((
                Status::Unauthorized,
                Json(ApiError {
                    error: "Invalid or expired admin session".to_string(),
                }),
            )),
            Err(e) => Outcome::Error((
                Status::InternalServerError,
                Json(ApiError {
                    error: format!("Admin session lookup failed: {e}"),
                }),
            )),
        }
    }
}

#[derive(Serialize)]
#[serde(crate = "rocket::serde")]
struct HealthResponse {
    status: &'static str,
    time_utc: String,
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct ConsentRequest {
    policy_version: String,
    privacy_notice_version: String,
    jurisdiction: String,
    marketing_opt_in: bool,
    analytics_opt_in: bool,
    personalization_opt_in: bool,
    privacy_notice_accepted: bool,
    terms_accepted: bool,
    data_processing_accepted: bool,
    source_ip: Option<String>,
}

#[derive(Serialize)]
#[serde(crate = "rocket::serde")]
struct ConsentResponse {
    consent_id: Uuid,
    created_at: String,
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct RegisterRequest {
    email: String,
    password: String,
    full_name: Option<String>,
    phone: Option<String>,
}

#[derive(Serialize)]
#[serde(crate = "rocket::serde")]
struct RegisterResponse {
    user_id: Uuid,
    api_token: String,
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct DataEventRequest {
    consent_id: Uuid,
    event_type: String,
    payload: serde_json::Value,
    user_id: Option<Uuid>,
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct FormSubmissionRequest {
    consent_id: Uuid,
    form_name: String,
    fields: serde_json::Value,
    user_id: Option<Uuid>,
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct DeletionRequestInput {
    consent_id: Option<Uuid>,
    user_id: Option<Uuid>,
    email: String,
    jurisdiction: String,
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct ExportRequestInput {
    consent_id: Option<Uuid>,
    user_id: Option<Uuid>,
    email: String,
    jurisdiction: String,
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct AdminLoginRequest {
    api_key: String,
}

#[derive(Serialize)]
#[serde(crate = "rocket::serde")]
struct AdminLoginResponse {
    session_token: String,
    expires_at: String,
}

#[derive(Serialize)]
#[serde(crate = "rocket::serde")]
struct AdminTablesResponse {
    tables: Vec<String>,
}

#[derive(Serialize)]
#[serde(crate = "rocket::serde")]
struct AdminTableRowsResponse {
    table: String,
    row_count: usize,
    rows: serde_json::Value,
}

#[derive(FromForm)]
struct TableQuery {
    limit: Option<i64>,
}

#[derive(Serialize)]
#[serde(crate = "rocket::serde")]
struct WorkflowRequestResponse {
    request_id: Uuid,
    status: &'static str,
}

#[derive(Serialize)]
#[serde(crate = "rocket::serde")]
struct RetentionRunResponse {
    ok: bool,
    deleted_event_rows: u64,
    deleted_form_rows: u64,
}

#[derive(Serialize)]
#[serde(crate = "rocket::serde")]
struct ExportProcessResponse {
    ok: bool,
    user: Option<serde_json::Value>,
    events: Vec<serde_json::Value>,
    forms: Vec<serde_json::Value>,
}

#[derive(Serialize)]
#[serde(crate = "rocket::serde")]
struct LegalTextResponse {
    privacy_notice: String,
    terms_of_use: String,
    retention_summary: String,
    jurisdictions: Vec<String>,
}

#[derive(Serialize)]
#[serde(crate = "rocket::serde")]
struct GenericOk {
    ok: bool,
}

#[derive(Debug, Serialize)]
#[serde(crate = "rocket::serde")]
struct ApiError {
    error: String,
}

struct Cors;

#[rocket::async_trait]
impl Fairing for Cors {
    fn info(&self) -> fairing::Info {
        fairing::Info {
            name: "CORS",
            kind: fairing::Kind::Response,
        }
    }

    async fn on_response<'r>(&self, _req: &'r Request<'_>, res: &mut Response<'r>) {
        res.set_header(Header::new("Access-Control-Allow-Origin", "*"));
        res.set_header(Header::new("Access-Control-Allow-Methods", "POST, GET, OPTIONS"));
        res.set_header(Header::new(
            "Access-Control-Allow-Headers",
            "Content-Type, Authorization, X-Admin-Key",
        ));
    }
}

#[options("/<_..>")]
fn options_route() {}

#[get("/api/health")]
fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        time_utc: Utc::now().to_rfc3339(),
    })
}

#[get("/api/compliance/legal-text")]
fn legal_text() -> Json<LegalTextResponse> {
    Json(LegalTextResponse {
        privacy_notice: "We collect data only after explicit consent. We process profile, telemetry, and interaction data for analytics, personalization, and communications based on your selected opt-ins. You can request export or deletion of personal data at any time.".to_string(),
        terms_of_use: "By using this portal, you confirm that submitted information is accurate and that you are authorized to submit it. You may withdraw consent and request deletion using the compliance forms below.".to_string(),
        retention_summary: "Default retention: EU/EEA 180 days, UK 365 days, US-CA 365 days, US-OTHER 540 days, OTHER 365 days. Admin retention jobs permanently delete expired records from event and form tables.".to_string(),
        jurisdictions: vec![
            "EU".to_string(),
            "UK".to_string(),
            "US-CA".to_string(),
            "US-OTHER".to_string(),
            "OTHER".to_string(),
        ],
    })
}

#[post("/api/admin/login", data = "<req>")]
async fn admin_login(
    state: &State<AppState>,
    req: Json<AdminLoginRequest>,
) -> Result<Json<AdminLoginResponse>, Json<ApiError>> {
    let expected = match &state.admin_api_key {
        Some(v) if !v.is_empty() => v,
        _ => {
            return Err(Json(ApiError {
                error: "ADMIN_API_KEY is not configured".to_string(),
            }))
        }
    };

    if req.api_key != *expected {
        return Err(Json(ApiError {
            error: "Invalid admin credentials".to_string(),
        }));
    }

    let session_token = generate_token();
    let expires_at = Utc::now() + chrono::Duration::hours(8);

    sqlx::query(
        r#"
        INSERT INTO admin_sessions (id, token, created_at, expires_at)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(&session_token)
    .bind(Utc::now())
    .bind(expires_at)
    .execute(&state.db)
    .await
    .map_err(|e| {
        Json(ApiError {
            error: format!("Unable to create admin session: {e}"),
        })
    })?;

    Ok(Json(AdminLoginResponse {
        session_token,
        expires_at: expires_at.to_rfc3339(),
    }))
}

#[get("/api/admin/tables")]
fn admin_tables(_session: AdminSession) -> Json<AdminTablesResponse> {
    Json(AdminTablesResponse {
        tables: allowed_admin_tables()
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
    })
}

#[get("/api/admin/tables/<table>?<query..>")]
async fn admin_table_rows(
    _session: AdminSession,
    state: &State<AppState>,
    table: String,
    query: TableQuery,
) -> Result<Json<AdminTableRowsResponse>, Json<ApiError>> {
    let table_name = table.to_lowercase();
    let base_select = table_select_query(&table_name).ok_or_else(|| {
        Json(ApiError {
            error: "Table is not allowed".to_string(),
        })
    })?;

    let requested_limit = query.limit.unwrap_or(200);
    let safe_limit = requested_limit.clamp(1, 2000);

    let wrapped = format!(
        "SELECT COALESCE(jsonb_agg(to_jsonb(t)), '[]'::jsonb) AS rows FROM ({base_select} LIMIT {safe_limit}) t"
    );

    let row = sqlx::query(&wrapped)
        .fetch_one(&state.db)
        .await
        .map_err(|e| {
            Json(ApiError {
                error: format!("Failed loading table data: {e}"),
            })
        })?;

    let rows_json: serde_json::Value = row.get("rows");
    let row_count = rows_json.as_array().map(|arr| arr.len()).unwrap_or(0);

    Ok(Json(AdminTableRowsResponse {
        table: table_name,
        row_count,
        rows: rows_json,
    }))
}

#[post("/api/consents", data = "<req>")]
async fn create_consent(
    state: &State<AppState>,
    req: Json<ConsentRequest>,
) -> Result<Json<ConsentResponse>, Json<ApiError>> {
    if !req.privacy_notice_accepted || !req.terms_accepted || !req.data_processing_accepted {
        return Err(Json(ApiError {
            error: "privacy_notice_accepted, terms_accepted, and data_processing_accepted must be true".to_string(),
        }));
    }

    let jurisdiction = normalize_jurisdiction(&req.jurisdiction);
    let consent_id = Uuid::new_v4();
    let created_at = Utc::now();

    let result = sqlx::query(
        r#"
        INSERT INTO consents (
            id, policy_version, privacy_notice_version, jurisdiction,
            marketing_opt_in, analytics_opt_in, personalization_opt_in,
            privacy_notice_accepted, terms_accepted, data_processing_accepted, source_ip, created_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        "#,
    )
    .bind(consent_id)
    .bind(&req.policy_version)
    .bind(&req.privacy_notice_version)
    .bind(jurisdiction)
    .bind(req.marketing_opt_in)
    .bind(req.analytics_opt_in)
    .bind(req.personalization_opt_in)
    .bind(req.privacy_notice_accepted)
    .bind(req.terms_accepted)
    .bind(req.data_processing_accepted)
    .bind(&req.source_ip)
    .bind(created_at)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => Ok(Json(ConsentResponse {
            consent_id,
            created_at: created_at.to_rfc3339(),
        })),
        Err(e) => Err(Json(ApiError {
            error: format!("Failed to create consent record: {e}"),
        })),
    }
}

#[post("/api/users/register", data = "<req>")]
async fn register_user(
    state: &State<AppState>,
    req: Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, Json<ApiError>> {
    let user_id = Uuid::new_v4();
    let token = generate_token();

    // Password is stored directly for demo/test purposes only.
    let result = sqlx::query(
        r#"
        INSERT INTO users (id, email, password, full_name, phone, api_token, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(user_id)
    .bind(&req.email)
    .bind(&req.password)
    .bind(&req.full_name)
    .bind(&req.phone)
    .bind(&token)
    .bind(Utc::now())
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => Ok(Json(RegisterResponse {
            user_id,
            api_token: token,
        })),
        Err(e) => Err(Json(ApiError {
            error: format!("Failed to register user: {e}"),
        })),
    }
}

#[post("/api/data/events", data = "<req>")]
async fn ingest_event(
    state: &State<AppState>,
    req: Json<DataEventRequest>,
) -> Result<Json<GenericOk>, Json<ApiError>> {
    if !consent_exists(&state.db, req.consent_id).await? {
        return Err(Json(ApiError {
            error: "Missing or invalid consent_id".to_string(),
        }));
    }

    let result = sqlx::query(
        r#"
        INSERT INTO data_events (id, consent_id, user_id, event_type, payload, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(req.consent_id)
    .bind(req.user_id)
    .bind(&req.event_type)
    .bind(&req.payload)
    .bind(Utc::now())
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => Ok(Json(GenericOk { ok: true })),
        Err(e) => Err(Json(ApiError {
            error: format!("Failed to ingest event: {e}"),
        })),
    }
}

#[post("/api/data/forms", data = "<req>")]
async fn submit_form(
    state: &State<AppState>,
    req: Json<FormSubmissionRequest>,
) -> Result<Json<GenericOk>, Json<ApiError>> {
    if !consent_exists(&state.db, req.consent_id).await? {
        return Err(Json(ApiError {
            error: "Missing or invalid consent_id".to_string(),
        }));
    }

    let result = sqlx::query(
        r#"
        INSERT INTO form_submissions (id, consent_id, user_id, form_name, fields, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(req.consent_id)
    .bind(req.user_id)
    .bind(&req.form_name)
    .bind(&req.fields)
    .bind(Utc::now())
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => Ok(Json(GenericOk { ok: true })),
        Err(e) => Err(Json(ApiError {
            error: format!("Failed to submit form: {e}"),
        })),
    }
}

#[post("/api/compliance/deletion-requests", data = "<req>")]
async fn create_deletion_request(
    state: &State<AppState>,
    req: Json<DeletionRequestInput>,
) -> Result<Json<WorkflowRequestResponse>, Json<ApiError>> {
    if req.consent_id.is_none() && req.user_id.is_none() {
        return Err(Json(ApiError {
            error: "Either consent_id or user_id is required".to_string(),
        }));
    }

    let request_id = Uuid::new_v4();
    let result = sqlx::query(
        r#"
        INSERT INTO data_deletion_requests (
            id, consent_id, user_id, email, jurisdiction, reason, status, requested_at
        ) VALUES ($1, $2, $3, $4, $5, $6, 'pending', $7)
        "#,
    )
    .bind(request_id)
    .bind(req.consent_id)
    .bind(req.user_id)
    .bind(&req.email)
    .bind(normalize_jurisdiction(&req.jurisdiction))
    .bind(&req.reason)
    .bind(Utc::now())
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => {
            log_audit(
                &state.db,
                "deletion_request_created",
                serde_json::json!({"request_id": request_id, "email": req.email}),
            )
            .await;
            Ok(Json(WorkflowRequestResponse {
                request_id,
                status: "pending",
            }))
        }
        Err(e) => Err(Json(ApiError {
            error: format!("Failed to create deletion request: {e}"),
        })),
    }
}

#[post("/api/compliance/export-requests", data = "<req>")]
async fn create_export_request(
    state: &State<AppState>,
    req: Json<ExportRequestInput>,
) -> Result<Json<WorkflowRequestResponse>, Json<ApiError>> {
    if req.consent_id.is_none() && req.user_id.is_none() {
        return Err(Json(ApiError {
            error: "Either consent_id or user_id is required".to_string(),
        }));
    }

    let request_id = Uuid::new_v4();
    let result = sqlx::query(
        r#"
        INSERT INTO data_export_requests (
            id, consent_id, user_id, email, jurisdiction, reason, status, requested_at
        ) VALUES ($1, $2, $3, $4, $5, $6, 'pending', $7)
        "#,
    )
    .bind(request_id)
    .bind(req.consent_id)
    .bind(req.user_id)
    .bind(&req.email)
    .bind(normalize_jurisdiction(&req.jurisdiction))
    .bind(&req.reason)
    .bind(Utc::now())
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => {
            log_audit(
                &state.db,
                "export_request_created",
                serde_json::json!({"request_id": request_id, "email": req.email}),
            )
            .await;
            Ok(Json(WorkflowRequestResponse {
                request_id,
                status: "pending",
            }))
        }
        Err(e) => Err(Json(ApiError {
            error: format!("Failed to create export request: {e}"),
        })),
    }
}

#[post("/api/compliance/deletion-requests/<request_id>/process")]
async fn process_deletion_request(
    _admin: AdminKey,
    state: &State<AppState>,
    request_id: String,
) -> Result<Json<GenericOk>, Json<ApiError>> {
    let request_id = Uuid::parse_str(&request_id).map_err(|_| {
        Json(ApiError {
            error: "Invalid request_id format".to_string(),
        })
    })?;

    let row = sqlx::query(
        r#"
        SELECT consent_id, user_id, status
        FROM data_deletion_requests
        WHERE id = $1
        "#,
    )
    .bind(request_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| Json(ApiError {
        error: format!("Failed to load deletion request: {e}"),
    }))?;

    let row = match row {
        Some(v) => v,
        None => {
            return Err(Json(ApiError {
                error: "Deletion request not found".to_string(),
            }))
        }
    };

    let status: String = row.get("status");
    if status != "pending" {
        return Err(Json(ApiError {
            error: format!("Deletion request is already {status}"),
        }));
    }

    let consent_id: Option<Uuid> = row.get("consent_id");
    let user_id: Option<Uuid> = row.get("user_id");

    if consent_id.is_none() && user_id.is_none() {
        return Err(Json(ApiError {
            error: "Deletion request missing both consent_id and user_id".to_string(),
        }));
    }

    if let Some(uid) = user_id {
        sqlx::query("DELETE FROM data_events WHERE user_id = $1")
            .bind(uid)
            .execute(&state.db)
            .await
            .map_err(|e| Json(ApiError {
                error: format!("Failed deleting user events: {e}"),
            }))?;

        sqlx::query("DELETE FROM form_submissions WHERE user_id = $1")
            .bind(uid)
            .execute(&state.db)
            .await
            .map_err(|e| Json(ApiError {
                error: format!("Failed deleting user forms: {e}"),
            }))?;

        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(uid)
            .execute(&state.db)
            .await
            .map_err(|e| Json(ApiError {
                error: format!("Failed deleting user row: {e}"),
            }))?;
    }

    if let Some(cid) = consent_id {
        sqlx::query("DELETE FROM data_events WHERE consent_id = $1")
            .bind(cid)
            .execute(&state.db)
            .await
            .map_err(|e| Json(ApiError {
                error: format!("Failed deleting consent events: {e}"),
            }))?;

        sqlx::query("DELETE FROM form_submissions WHERE consent_id = $1")
            .bind(cid)
            .execute(&state.db)
            .await
            .map_err(|e| Json(ApiError {
                error: format!("Failed deleting consent forms: {e}"),
            }))?;
    }

    sqlx::query("UPDATE data_deletion_requests SET status = 'completed', processed_at = $2 WHERE id = $1")
        .bind(request_id)
        .bind(Utc::now())
        .execute(&state.db)
        .await
        .map_err(|e| Json(ApiError {
            error: format!("Failed updating request status: {e}"),
        }))?;

    log_audit(
        &state.db,
        "deletion_request_processed",
        serde_json::json!({"request_id": request_id, "consent_id": consent_id, "user_id": user_id}),
    )
    .await;

    Ok(Json(GenericOk { ok: true }))
}

#[post("/api/compliance/export-requests/<request_id>/process")]
async fn process_export_request(
    _admin: AdminKey,
    state: &State<AppState>,
    request_id: String,
) -> Result<Json<ExportProcessResponse>, Json<ApiError>> {
    let request_id = Uuid::parse_str(&request_id).map_err(|_| {
        Json(ApiError {
            error: "Invalid request_id format".to_string(),
        })
    })?;

    let row = sqlx::query(
        r#"
        SELECT consent_id, user_id, status
        FROM data_export_requests
        WHERE id = $1
        "#,
    )
    .bind(request_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| Json(ApiError {
        error: format!("Failed to load export request: {e}"),
    }))?;

    let row = match row {
        Some(v) => v,
        None => {
            return Err(Json(ApiError {
                error: "Export request not found".to_string(),
            }))
        }
    };

    let status: String = row.get("status");
    if status != "pending" {
        return Err(Json(ApiError {
            error: format!("Export request is already {status}"),
        }));
    }

    let consent_id: Option<Uuid> = row.get("consent_id");
    let user_id: Option<Uuid> = row.get("user_id");

    if consent_id.is_none() && user_id.is_none() {
        return Err(Json(ApiError {
            error: "Export request missing both consent_id and user_id".to_string(),
        }));
    }

    let user = if let Some(uid) = user_id {
        sqlx::query(
            "SELECT id, email, full_name, phone, created_at FROM users WHERE id = $1",
        )
        .bind(uid)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| Json(ApiError {
            error: format!("Failed loading user for export: {e}"),
        }))?
        .map(|r| {
            serde_json::json!({
                "id": r.get::<Uuid, _>("id"),
                "email": r.get::<String, _>("email"),
                "full_name": r.get::<Option<String>, _>("full_name"),
                "phone": r.get::<Option<String>, _>("phone"),
                "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at").to_rfc3339(),
            })
        })
    } else {
        None
    };

    let events_rows = if let Some(uid) = user_id {
        sqlx::query(
            "SELECT id, consent_id, user_id, event_type, payload, created_at FROM data_events WHERE user_id = $1 ORDER BY created_at DESC",
        )
        .bind(uid)
        .fetch_all(&state.db)
        .await
        .map_err(|e| Json(ApiError {
            error: format!("Failed loading event export by user: {e}"),
        }))?
    } else {
        sqlx::query(
            "SELECT id, consent_id, user_id, event_type, payload, created_at FROM data_events WHERE consent_id = $1 ORDER BY created_at DESC",
        )
        .bind(consent_id.expect("consent id checked above"))
        .fetch_all(&state.db)
        .await
        .map_err(|e| Json(ApiError {
            error: format!("Failed loading event export by consent: {e}"),
        }))?
    };

    let form_rows = if let Some(uid) = user_id {
        sqlx::query(
            "SELECT id, consent_id, user_id, form_name, fields, created_at FROM form_submissions WHERE user_id = $1 ORDER BY created_at DESC",
        )
        .bind(uid)
        .fetch_all(&state.db)
        .await
        .map_err(|e| Json(ApiError {
            error: format!("Failed loading form export by user: {e}"),
        }))?
    } else {
        sqlx::query(
            "SELECT id, consent_id, user_id, form_name, fields, created_at FROM form_submissions WHERE consent_id = $1 ORDER BY created_at DESC",
        )
        .bind(consent_id.expect("consent id checked above"))
        .fetch_all(&state.db)
        .await
        .map_err(|e| Json(ApiError {
            error: format!("Failed loading form export by consent: {e}"),
        }))?
    };

    let events: Vec<serde_json::Value> = events_rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<Uuid, _>("id"),
                "consent_id": r.get::<Uuid, _>("consent_id"),
                "user_id": r.get::<Option<Uuid>, _>("user_id"),
                "event_type": r.get::<String, _>("event_type"),
                "payload": r.get::<serde_json::Value, _>("payload"),
                "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at").to_rfc3339(),
            })
        })
        .collect();

    let forms: Vec<serde_json::Value> = form_rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<Uuid, _>("id"),
                "consent_id": r.get::<Uuid, _>("consent_id"),
                "user_id": r.get::<Option<Uuid>, _>("user_id"),
                "form_name": r.get::<String, _>("form_name"),
                "fields": r.get::<serde_json::Value, _>("fields"),
                "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at").to_rfc3339(),
            })
        })
        .collect();

    sqlx::query("UPDATE data_export_requests SET status = 'completed', processed_at = $2 WHERE id = $1")
        .bind(request_id)
        .bind(Utc::now())
        .execute(&state.db)
        .await
        .map_err(|e| Json(ApiError {
            error: format!("Failed updating export request status: {e}"),
        }))?;

    log_audit(
        &state.db,
        "export_request_processed",
        serde_json::json!({"request_id": request_id, "event_count": events.len(), "form_count": forms.len()}),
    )
    .await;

    Ok(Json(ExportProcessResponse {
        ok: true,
        user,
        events,
        forms,
    }))
}

#[post("/api/compliance/retention/run")]
async fn run_retention(_admin: AdminKey, state: &State<AppState>) -> Result<Json<RetentionRunResponse>, Json<ApiError>> {
    let jurisdictions = ["EU", "UK", "US-CA", "US-OTHER", "OTHER"];
    let mut deleted_events = 0u64;
    let mut deleted_forms = 0u64;

    for jurisdiction in jurisdictions {
        let days = retention_days(jurisdiction);

        let events = sqlx::query(
            r#"
            DELETE FROM data_events e
            USING consents c
            WHERE e.consent_id = c.id
              AND c.jurisdiction = $1
              AND e.created_at < NOW() - make_interval(days => $2)
            "#,
        )
        .bind(jurisdiction)
        .bind(days)
        .execute(&state.db)
        .await
        .map_err(|e| Json(ApiError {
            error: format!("Retention event cleanup failed for {jurisdiction}: {e}"),
        }))?;

        let forms = sqlx::query(
            r#"
            DELETE FROM form_submissions f
            USING consents c
            WHERE f.consent_id = c.id
              AND c.jurisdiction = $1
              AND f.created_at < NOW() - make_interval(days => $2)
            "#,
        )
        .bind(jurisdiction)
        .bind(days)
        .execute(&state.db)
        .await
        .map_err(|e| Json(ApiError {
            error: format!("Retention form cleanup failed for {jurisdiction}: {e}"),
        }))?;

        deleted_events += events.rows_affected();
        deleted_forms += forms.rows_affected();
    }

    log_audit(
        &state.db,
        "retention_run",
        serde_json::json!({
            "deleted_event_rows": deleted_events,
            "deleted_form_rows": deleted_forms
        }),
    )
    .await;

    Ok(Json(RetentionRunResponse {
        ok: true,
        deleted_event_rows: deleted_events,
        deleted_form_rows: deleted_forms,
    }))
}

fn retention_days(jurisdiction: &str) -> i64 {
    match jurisdiction {
        "EU" => 180,
        "UK" => 365,
        "US-CA" => 365,
        "US-OTHER" => 540,
        _ => 365,
    }
}

fn allowed_admin_tables() -> &'static [&'static str] {
    &[
        "users",
        "consents",
        "data_events",
        "form_submissions",
        "data_deletion_requests",
        "data_export_requests",
        "compliance_audit_logs",
        "admin_sessions",
    ]
}

fn table_select_query(table: &str) -> Option<&'static str> {
    match table {
        // Intentionally excludes password.
        "users" => Some(
            "SELECT id, email, full_name, phone, api_token, created_at FROM users ORDER BY created_at DESC",
        ),
        "consents" => Some("SELECT * FROM consents ORDER BY created_at DESC"),
        "data_events" => Some("SELECT * FROM data_events ORDER BY created_at DESC"),
        "form_submissions" => Some("SELECT * FROM form_submissions ORDER BY created_at DESC"),
        "data_deletion_requests" => {
            Some("SELECT * FROM data_deletion_requests ORDER BY requested_at DESC")
        }
        "data_export_requests" => Some("SELECT * FROM data_export_requests ORDER BY requested_at DESC"),
        "compliance_audit_logs" => Some("SELECT * FROM compliance_audit_logs ORDER BY created_at DESC"),
        "admin_sessions" => Some("SELECT id, token, created_at, expires_at FROM admin_sessions ORDER BY created_at DESC"),
        _ => None,
    }
}

fn normalize_jurisdiction(input: &str) -> &'static str {
    match input {
        "EU" => "EU",
        "UK" => "UK",
        "US-CA" => "US-CA",
        "US-OTHER" => "US-OTHER",
        _ => "OTHER",
    }
}

async fn consent_exists(db: &PgPool, consent_id: Uuid) -> Result<bool, Json<ApiError>> {
    let row = sqlx::query("SELECT id FROM consents WHERE id = $1")
        .bind(consent_id)
        .fetch_optional(db)
        .await
        .map_err(|e| {
            Json(ApiError {
                error: format!("Consent lookup failed: {e}"),
            })
        })?;

    Ok(row.is_some())
}

async fn log_audit(db: &PgPool, action: &str, details: serde_json::Value) {
    let _ = sqlx::query(
        r#"
        INSERT INTO compliance_audit_logs (id, action, details, created_at)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(action)
    .bind(details)
    .bind(Utc::now())
    .execute(db)
    .await;
}

fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

async fn build_rocket() -> Result<Rocket<Build>, String> {
    dotenvy::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://collector:collector@localhost:5432/collector_test".to_string());

    let admin_api_key = std::env::var("ADMIN_API_KEY").ok();

    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .map_err(|e| format!("Unable to connect to database: {e}"))?;

    sqlx::query("SELECT 1")
        .fetch_one(&db)
        .await
        .map_err(|e| format!("Database ping failed: {e}"))?;

    Ok(rocket::build()
        .attach(Cors)
        .manage(AppState { db, admin_api_key })
        .mount(
            "/",
            routes![
                health,
                legal_text,
                admin_login,
                admin_tables,
                admin_table_rows,
                create_consent,
                register_user,
                ingest_event,
                submit_form,
                create_deletion_request,
                create_export_request,
                process_deletion_request,
                process_export_request,
                run_retention,
                options_route
            ],
        ))
}

#[rocket::main]
async fn main() {
    match build_rocket().await {
        Ok(rocket) => {
            if let Err(err) = rocket.launch().await {
                eprintln!("Server failed: {err}");
            }
        }
        Err(err) => {
            eprintln!("Startup failed: {err}");
        }
    }
}
