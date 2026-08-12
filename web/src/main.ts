import { createApp } from 'vue'
import { createPinia } from 'pinia'
import { registerSW } from 'virtual:pwa-register'
import App from './App.vue'
import { router } from './router'
import './styles.css'

const savedTheme = localStorage.getItem('lumeno-theme')
if (savedTheme === 'light' || savedTheme === 'dark') document.documentElement.dataset.theme = savedTheme
registerSW({ immediate: true })
createApp(App).use(createPinia()).use(router).mount('#app')
