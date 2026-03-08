const API_BASE = import.meta.env.VITE_API_BASE ?? 'http://127.0.0.1:8000'

export type ConsentPayload = {
  policy_version: string
  privacy_notice_version: string
  jurisdiction: string
  marketing_opt_in: boolean
  analytics_opt_in: boolean
  personalization_opt_in: boolean
  privacy_notice_accepted: boolean
  terms_accepted: boolean
  data_processing_accepted: boolean
  source_ip?: string
}

export type RegisterPayload = {
  email: string
  password: string
  full_name?: string
  phone?: string
}

export async function getLegalText() {
  const res = await fetch(`${API_BASE}/api/compliance/legal-text`)
  if (!res.ok) throw new Error(`Legal text fetch failed: ${await res.text()}`)
  return res.json()
}

export async function createConsent(payload: ConsentPayload) {
  const res = await fetch(`${API_BASE}/api/consents`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload)
  })

  if (!res.ok) throw new Error(`Consent failed: ${await res.text()}`)
  const data = await res.json()
  const normalizedConsentId =
    data?.consent_id ??
    data?.consentId ??
    data?.consent?.id ??
    data?.consent?.consent_id ??
    data?.data?.consent_id ??
    data?.data?.consentId
  if (normalizedConsentId) {
    data.consent_id = normalizedConsentId
    data.consentId = normalizedConsentId
  }
  return data
}

export async function registerUser(payload: RegisterPayload) {
  const res = await fetch(`${API_BASE}/api/users/register`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload)
  })

  if (!res.ok) throw new Error(`Signup failed: ${await res.text()}`)
  const data = await res.json()
  const normalizedUserId =
    data?.user_id ??
    data?.userId ??
    data?.user?.id ??
    data?.data?.user_id ??
    data?.data?.userId
  if (normalizedUserId) {
    data.user_id = normalizedUserId
    data.userId = normalizedUserId
  }
  return data
}

export async function submitEvent(payload: {
  consent_id: string
  event_type: string
  payload: Record<string, unknown>
  user_id?: string
}) {
  const res = await fetch(`${API_BASE}/api/data/events`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload)
  })

  if (!res.ok) throw new Error(`Event submit failed: ${await res.text()}`)
  return res.json()
}

export async function submitForm(payload: {
  consent_id: string
  form_name: string
  fields: Record<string, unknown>
  user_id?: string
}) {
  const res = await fetch(`${API_BASE}/api/data/forms`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload)
  })

  if (!res.ok) throw new Error(`Form submit failed: ${await res.text()}`)
  return res.json()
}

export async function requestDeletion(payload: {
  consent_id?: string
  user_id?: string
  email: string
  jurisdiction: string
  reason?: string
}) {
  const res = await fetch(`${API_BASE}/api/compliance/deletion-requests`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload)
  })

  if (!res.ok) throw new Error(`Deletion request failed: ${await res.text()}`)
  return res.json()
}

export async function requestExport(payload: {
  consent_id?: string
  user_id?: string
  email: string
  jurisdiction: string
  reason?: string
}) {
  const res = await fetch(`${API_BASE}/api/compliance/export-requests`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload)
  })

  if (!res.ok) throw new Error(`Export request failed: ${await res.text()}`)
  return res.json()
}

export async function adminLogin(apiKey: string) {
  const res = await fetch(`${API_BASE}/api/admin/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ api_key: apiKey })
  })

  if (!res.ok) throw new Error(`Admin login failed: ${await res.text()}`)
  return res.json()
}

export async function adminListTables(sessionToken: string) {
  const res = await fetch(`${API_BASE}/api/admin/tables`, {
    headers: {
      Authorization: `Bearer ${sessionToken}`
    }
  })

  if (!res.ok) throw new Error(`Admin table list failed: ${await res.text()}`)
  return res.json()
}

export async function adminGetTableRows(sessionToken: string, table: string, limit = 200) {
  const res = await fetch(`${API_BASE}/api/admin/tables/${encodeURIComponent(table)}?limit=${limit}`, {
    headers: {
      Authorization: `Bearer ${sessionToken}`
    }
  })

  if (!res.ok) throw new Error(`Admin table fetch failed: ${await res.text()}`)
  return res.json()
}
