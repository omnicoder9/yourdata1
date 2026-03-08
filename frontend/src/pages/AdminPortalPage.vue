<template>
  <main class="layout">
    <section class="card">
      <h1>Admin Portal</h1>
      <p>Login and view database tables. User passwords are not exposed by the API.</p>

      <div class="group" v-if="!sessionToken">
        <h2>Admin Login</h2>
        <input v-model="apiKey" type="password" placeholder="Admin API key" />
        <button :disabled="loading || !apiKey" @click="onLogin">Login</button>
      </div>

      <div class="group" v-else>
        <h2>Data Browser</h2>
        <label>
          Table
          <select v-model="selectedTable" @change="onLoadTable">
            <option v-for="table in tables" :key="table" :value="table">{{ table }}</option>
          </select>
        </label>
        <label>
          Row Limit
          <input v-model.number="limit" type="number" min="1" max="2000" />
        </label>
        <div class="buttons">
          <button :disabled="loading || !selectedTable" @click="onLoadTable">Load table</button>
          <button :disabled="loading" @click="onLogout">Logout</button>
        </div>
      </div>

      <div class="group" v-if="rows.length > 0">
        <h2>Rows ({{ rows.length }})</h2>
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th v-for="column in columns" :key="column">{{ column }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="(row, idx) in rows" :key="idx">
                <td v-for="column in columns" :key="column">
                  {{ formatCell(row[column]) }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      <div class="group" v-if="rows.length > 0">
        <h2>Raw JSON</h2>
        <pre class="json-box">{{ JSON.stringify(rows, null, 2) }}</pre>
      </div>

      <p v-if="error" class="error">{{ error }}</p>
      <p v-if="success" class="ok">{{ success }}</p>
    </section>
  </main>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { adminGetTableRows, adminListTables, adminLogin } from '../services/api'

const loading = ref(false)
const error = ref('')
const success = ref('')
const apiKey = ref('')
const sessionToken = ref('')
const tables = ref<string[]>([])
const selectedTable = ref('')
const rows = ref<Record<string, unknown>[]>([])
const limit = ref(200)

const columns = computed(() => {
  if (rows.value.length === 0) return []
  return Object.keys(rows.value[0])
})

onMounted(async () => {
  const stored = localStorage.getItem('admin_session_token')
  if (!stored) return

  sessionToken.value = stored
  await loadTables()
})

async function onLogin() {
  loading.value = true
  error.value = ''
  success.value = ''

  try {
    const res = await adminLogin(apiKey.value)
    sessionToken.value = res.session_token
    localStorage.setItem('admin_session_token', res.session_token)
    await loadTables()
    success.value = 'Admin login successful.'
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Unknown admin login error'
  } finally {
    loading.value = false
  }
}

async function loadTables() {
  if (!sessionToken.value) return

  try {
    const res = await adminListTables(sessionToken.value)
    tables.value = res.tables ?? []
    if (tables.value.length > 0) {
      selectedTable.value = tables.value[0]
      await onLoadTable()
    }
  } catch (e) {
    onLogout()
    error.value = e instanceof Error ? e.message : 'Unable to load tables'
  }
}

async function onLoadTable() {
  if (!sessionToken.value || !selectedTable.value) return

  loading.value = true
  error.value = ''
  success.value = ''

  try {
    const safeLimit = Math.max(1, Math.min(2000, Number(limit.value) || 200))
    const res = await adminGetTableRows(sessionToken.value, selectedTable.value, safeLimit)
    rows.value = Array.isArray(res.rows) ? res.rows : []
    success.value = `Loaded ${rows.value.length} row(s) from ${selectedTable.value}.`
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Unknown admin table error'
  } finally {
    loading.value = false
  }
}

function onLogout() {
  localStorage.removeItem('admin_session_token')
  sessionToken.value = ''
  rows.value = []
  tables.value = []
  selectedTable.value = ''
}

function formatCell(value: unknown) {
  if (value === null || value === undefined) return ''
  if (typeof value === 'object') return JSON.stringify(value)
  return String(value)
}
</script>
