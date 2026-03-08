import { createRouter, createWebHistory } from 'vue-router'
import CollectorPage from './pages/CollectorPage.vue'
import DataUseExplainerPage from './pages/DataUseExplainerPage.vue'
import AdminPortalPage from './pages/AdminPortalPage.vue'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', name: 'collector', component: CollectorPage },
    { path: '/data-use-explainer', name: 'data-use-explainer', component: DataUseExplainerPage },
    { path: '/admin', name: 'admin', component: AdminPortalPage }
  ]
})

export default router
