import { createRouter, createWebHistory } from 'vue-router'
import TodayView from './views/TodayView.vue'
import ChainsView from './views/ChainsView.vue'
import ArchivedChainsView from './views/ArchivedChainsView.vue'
import GuideView from './views/GuideView.vue'
import RsipView from './views/RsipView.vue'
import HistoryView from './views/HistoryView.vue'
import SettingsView from './views/SettingsView.vue'

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', name: 'today', component: TodayView },
    { path: '/chains', name: 'chains', component: ChainsView },
    { path: '/chains/archive', name: 'archived-chains', component: ArchivedChainsView },
    { path: '/rsip', name: 'rsip', component: RsipView },
    { path: '/history', name: 'history', component: HistoryView },
    { path: '/guide', name: 'guide', component: GuideView },
    { path: '/settings', name: 'settings', component: SettingsView },
  ],
})
