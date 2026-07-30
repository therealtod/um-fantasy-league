import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import router from './router'
import { useAuthStore } from '@/stores/auth'
import './assets/main.css'

const app = createApp(App)
app.use(createPinia())
app.use(router)

// The router guard reads the session synchronously, so it must be resolved
// before the first navigation runs.
useAuthStore()
  .init()
  .then(() => app.mount('#app'))
