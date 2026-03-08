<template>
  <main class="layout">
    <section class="card">
      <h1>Research Data Collection Portal</h1>
      <p>
        This app collects user-provided and device metadata only after explicit consent.
        Account creation is optional and increases personalized data collection scope.
      </p>

      <div class="group legal" v-if="legalText">
        <h2>Legal Notice</h2>
        <p><strong>Privacy Notice</strong> {{ legalText.privacy_notice }}</p>
        <p><strong>Terms of Use</strong> {{ legalText.terms_of_use }}</p>
        <p><strong>Retention Summary</strong> {{ legalText.retention_summary }}</p>
      </div>

      <div class="group">
        <h2>1. Consent</h2>
        <label>
          Jurisdiction
          <select v-model="consent.jurisdiction">
            <option value="EU">EU</option>
            <option value="UK">UK</option>
            <option value="US-CA">US-CA</option>
            <option value="US-OTHER">US-OTHER</option>
            <option value="OTHER">OTHER</option>
          </select>
        </label>

        <label><input type="checkbox" v-model="consent.marketing_opt_in" /> Marketing data</label>
        <label><input type="checkbox" v-model="consent.analytics_opt_in" /> Analytics data</label>
        <label><input type="checkbox" v-model="consent.personalization_opt_in" /> Personalization data</label>

        <label>
          <input type="checkbox" v-model="privacyAccepted" />
          I acknowledge privacy notice version {{ consent.privacy_notice_version }}.
        </label>
        <label>
          <input type="checkbox" v-model="termsAccepted" />
          I accept terms of use version {{ consent.policy_version }}.
        </label>
        <label>
          <input type="checkbox" v-model="processingAccepted" />
          I consent to data processing for selected purposes.
        </label>

        <button :disabled="loading || !canSubmitConsent" @click="onConsent">Save consent</button>
        <p v-if="consentId" class="ok">Consent recorded: {{ consentId }}</p>
      </div>

      <div class="group" v-if="consentId">
        <h2>2. Optional account</h2>
        <input v-model="account.email" type="email" placeholder="Email" />
        <input v-model="account.password" type="password" placeholder="Password" />
        <input v-model="account.full_name" type="text" placeholder="Full name" />
        <input v-model="account.phone" type="tel" placeholder="Phone" />

        <button :disabled="loading || !account.email || !account.password" @click="onRegister">
          Create optional account
        </button>
        <p v-if="userId" class="ok">User created: {{ userId }}</p>
      </div>

      <div class="group" v-if="consentId">
        <h2>3. Extended profile form</h2>
        <input v-model="profile.age_range" placeholder="Age range (e.g. 25-34)" />
        <input v-model="profile.city" placeholder="City" />
        <input v-model="profile.country" placeholder="Country" />
        <input v-model="profile.job_title" placeholder="Job title" />
        <input v-model="profile.company" placeholder="Company" />
        <textarea v-model="profile.interests" placeholder="Interests (comma-separated)"></textarea>
        <textarea v-model="profile.notes" placeholder="Additional notes"></textarea>
        <button :disabled="loading" @click="onSubmitProfile">Submit profile</button>
      </div>

      <div class="group" v-if="consentId">
        <h2>4. Data rights workflows</h2>
        <p>Submit requests for export or deletion based on your jurisdiction.</p>
        <input v-model="rights.email" type="email" placeholder="Contact email for request" />
        <textarea v-model="rights.reason" placeholder="Request reason/details"></textarea>
        <div class="buttons">
          <button :disabled="loading || !rights.email" @click="onRequestExport">Request data export</button>
          <button :disabled="loading || !rights.email" @click="onRequestDeletion">Request data deletion</button>
        </div>
      </div>

      <p v-if="error" class="error">{{ error }}</p>
      <p v-if="success" class="ok">{{ success }}</p>
    </section>
  </main>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import {
  createConsent,
  getLegalText,
  registerUser,
  requestDeletion,
  requestExport,
  submitEvent,
  submitForm
} from '../services/api'

const loading = ref(false)
const error = ref('')
const success = ref('')
const consentId = ref('')
const userId = ref('')

const privacyAccepted = ref(false)
const termsAccepted = ref(false)
const processingAccepted = ref(false)

const legalText = ref<null | {
  privacy_notice: string
  terms_of_use: string
  retention_summary: string
  jurisdictions: string[]
}>(null)

const consent = reactive({
  policy_version: 'v1.0.0',
  privacy_notice_version: 'privacy-v1.0.0',
  jurisdiction: 'US-OTHER',
  marketing_opt_in: true,
  analytics_opt_in: true,
  personalization_opt_in: true
})

const account = reactive({
  email: '',
  password: '',
  full_name: '',
  phone: ''
})

const profile = reactive({
  age_range: '',
  city: '',
  country: '',
  job_title: '',
  company: '',
  interests: '',
  notes: ''
})

const rights = reactive({
  email: '',
  reason: ''
})

const canSubmitConsent = computed(
  () => privacyAccepted.value && termsAccepted.value && processingAccepted.value
)

onMounted(async () => {
  try {
    legalText.value = await getLegalText()
  } catch {
    // Legal text fetch failure should not block consent flow in test mode.
  }
})

function collectAutoData() {
  return {
    userAgent: navigator.userAgent,
    language: navigator.language,
    platform: navigator.platform,
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    screen: `${window.screen.width}x${window.screen.height}`,
    referrer: document.referrer || null,
    pageUrl: window.location.href,
    ts: new Date().toISOString()
  }
}

async function onConsent() {
  loading.value = true
  error.value = ''
  success.value = ''

  try {
    const consentRes = await createConsent({
      ...consent,
      privacy_notice_accepted: privacyAccepted.value,
      terms_accepted: termsAccepted.value,
      data_processing_accepted: processingAccepted.value
    })
    const createdConsentId = consentRes?.consent_id ?? consentRes?.consentId
    if (!createdConsentId) {
      throw new Error(
        `Consent API did not return a consent_id. Payload: ${JSON.stringify(consentRes)}`
      )
    }
    consentId.value = createdConsentId

    await submitEvent({
      consent_id: createdConsentId,
      event_type: 'client_bootstrap',
      payload: collectAutoData()
    })

    success.value = 'Consent saved and initial telemetry captured.'
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Unknown consent error'
  } finally {
    loading.value = false
  }
}

async function onRegister() {
  loading.value = true
  error.value = ''
  success.value = ''

  try {
    const user = await registerUser(account)
    const createdUserId = user?.user_id ?? user?.userId
    if (!createdUserId) {
      throw new Error(`Register API did not return a user_id. Payload: ${JSON.stringify(user)}`)
    }
    userId.value = createdUserId

    if (consentId.value) {
      await submitEvent({
        consent_id: consentId.value,
        user_id: userId.value,
        event_type: 'user_registered',
        payload: {
          email_domain: account.email.split('@')[1] || null,
          has_phone: Boolean(account.phone)
        }
      })
    }

    success.value = 'Optional account created.'
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Unknown signup error'
  } finally {
    loading.value = false
  }
}

async function onSubmitProfile() {
  if (!consentId.value) {
    error.value = 'Consent is required before profile submission.'
    return
  }

  loading.value = true
  error.value = ''
  success.value = ''

  try {
    await submitForm({
      consent_id: consentId.value,
      user_id: userId.value || undefined,
      form_name: 'extended_profile',
      fields: {
        ...profile,
        interests_list: profile.interests
          .split(',')
          .map((item) => item.trim())
          .filter(Boolean)
      }
    })

    success.value = 'Profile submitted.'
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Unknown form error'
  } finally {
    loading.value = false
  }
}

async function onRequestExport() {
  loading.value = true
  error.value = ''
  success.value = ''

  try {
    const res = await requestExport({
      consent_id: consentId.value,
      user_id: userId.value || undefined,
      email: rights.email,
      jurisdiction: consent.jurisdiction,
      reason: rights.reason || undefined
    })

    success.value = `Export request submitted with id ${res.request_id}.`
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Unknown export request error'
  } finally {
    loading.value = false
  }
}

async function onRequestDeletion() {
  loading.value = true
  error.value = ''
  success.value = ''

  try {
    const res = await requestDeletion({
      consent_id: consentId.value,
      user_id: userId.value || undefined,
      email: rights.email,
      jurisdiction: consent.jurisdiction,
      reason: rights.reason || undefined
    })

    success.value = `Deletion request submitted with id ${res.request_id}.`
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Unknown deletion request error'
  } finally {
    loading.value = false
  }
}
</script>
