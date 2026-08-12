<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Bell, FlaskConical, LogOut, Moon, RefreshCw, Save, Sun, Trash2 } from '@lucide/vue'
import { api, mutation } from '../lib/api'
import { useAppStore } from '../stores/app'
import type { NotificationChannel, NotificationChannelKind } from '../types'

const app = useAppStore()
const timezone = computed(() => app.me?.timezone ?? '正在读取…')
const theme = ref(localStorage.getItem('lumeno-theme') ?? 'system')
const channel = ref<NotificationChannel | null>(null)
const channelKind = ref<NotificationChannelKind>('bark')
const endpoint = ref('')
const bearerToken = ref('')
const barkEncryptionEnabled = ref(false)
const barkEncryptionKey = ref('')
const notificationBusy = ref(false)
const notificationState = ref('')

const channelSummary = computed(() => {
  if (!channel.value?.configured) return '尚未配置'
  const name = channel.value.kind === 'bark' ? 'Bark' : 'Webhook'
  const details = [channel.value.endpointHint]
  if (channel.value.barkEncryptionEnabled) details.push('内容加密已启用')
  if (channel.value.hasBearerToken) details.push('认证令牌已保存')
  return `${name} · ${details.filter(Boolean).join(' · ')}`
})

onMounted(loadNotificationChannel)

function setTheme(value: string) {
  theme.value = value
  localStorage.setItem('lumeno-theme', value)
  document.documentElement.dataset.theme = value === 'system' ? '' : value
}

async function generateBarkEncryptionKey() {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_'
  const random = crypto.getRandomValues(new Uint8Array(32))
  barkEncryptionKey.value = [...random].map((value) => alphabet[value & 63]).join('')
  try {
    await navigator.clipboard.writeText(barkEncryptionKey.value)
    notificationState.value = '已生成密钥并复制到剪贴板'
  } catch {
    notificationState.value = '已生成密钥，请从输入框复制'
  }
}

async function loadNotificationChannel() {
  try {
    channel.value = await api<NotificationChannel>('/api/v1/notifications/channel')
    if (channel.value.kind) channelKind.value = channel.value.kind
    barkEncryptionEnabled.value = channel.value.barkEncryptionEnabled
  } catch (error) { app.report(error) }
}

async function saveNotificationChannel() {
  notificationBusy.value = true
  notificationState.value = ''
  try {
    channel.value = await mutation<NotificationChannel>('/api/v1/notifications/channel', 'PUT', {
      kind: channelKind.value,
      endpoint: endpoint.value,
      bearerToken: channelKind.value === 'webhook' ? bearerToken.value || undefined : undefined,
      barkEncryptionKey: channelKind.value === 'bark' && barkEncryptionEnabled.value
        ? barkEncryptionKey.value
        : undefined,
    })
    endpoint.value = ''
    bearerToken.value = ''
    barkEncryptionKey.value = ''
    notificationState.value = '配置已保存'
  } catch (error) { app.report(error) } finally { notificationBusy.value = false }
}

async function testNotificationChannel() {
  notificationBusy.value = true
  notificationState.value = ''
  try {
    await mutation('/api/v1/notifications/test', 'POST')
    notificationState.value = '测试通知已发送'
  } catch (error) { app.report(error) } finally { notificationBusy.value = false }
}

async function deleteNotificationChannel() {
  notificationBusy.value = true
  notificationState.value = ''
  try {
    await mutation('/api/v1/notifications/channel', 'DELETE')
    channel.value = { configured: false, kind: null, endpointHint: null, hasBearerToken: false, barkEncryptionEnabled: false }
    endpoint.value = ''
    bearerToken.value = ''
    barkEncryptionEnabled.value = false
    barkEncryptionKey.value = ''
    notificationState.value = '通知渠道已删除'
  } catch (error) { app.report(error) } finally { notificationBusy.value = false }
}

async function logout() {
  try { const result = await mutation<{ redirectTo: string }>('/api/v1/auth/logout', 'POST'); window.location.assign(result.redirectTo) } catch (error) { app.report(error) }
}
</script>

<template>
  <div class="page narrow-page">
    <header class="page-header"><div><p class="eyebrow">PREFERENCES</p><h1>设置</h1><p>管理外观、通知和账号。</p></div></header>
    <section class="settings-card"><div><h2>时区</h2><p>用于计算每日激活次数。</p></div><div class="detected-timezone"><span>当前时区</span><strong>{{ timezone }}</strong></div></section>
    <section class="settings-card"><div><h2>界面外观</h2><p>选择你习惯的显示方式。</p></div><div class="theme-buttons"><button :class="{ selected: theme === 'light' }" @click="setTheme('light')"><Sun :size="16" /> 浅色</button><button :class="{ selected: theme === 'dark' }" @click="setTheme('dark')"><Moon :size="16" /> 深色</button><button :class="{ selected: theme === 'system' }" @click="setTheme('system')"><RefreshCw :size="16" /> 跟随系统</button></div></section>

    <section class="settings-card notification-card">
      <div class="notification-heading">
        <div><h2>通知</h2><p>在预约或计时结束时接收提醒。</p></div>
        <span class="notification-summary"><Bell :size="14" /> {{ channelSummary }}</span>
      </div>

      <form class="notification-form" @submit.prevent="saveNotificationChannel">
        <div class="segmented channel-selector" aria-label="通知渠道">
          <button type="button" :class="{ active: channelKind === 'bark' }" @click="channelKind = 'bark'">Bark</button>
          <button type="button" :class="{ active: channelKind === 'webhook' }" @click="channelKind = 'webhook'">自定义 Webhook</button>
        </div>

        <label v-if="channelKind === 'bark'">
          Bark 推送地址
          <input
            v-model="endpoint"
            required
            type="url"
            inputmode="url"
            autocomplete="off"
            placeholder="https://api.day.app/你的设备密钥"
          />
          <small>填写 Bark 提供的完整推送地址；其来源地址需由部署者加入白名单。</small>
        </label>
        <label v-else>
          Webhook 地址
          <input
            v-model="endpoint"
            required
            type="url"
            inputmode="url"
            autocomplete="off"
            placeholder="https://example.com/hooks/lumeno"
          />
          <small>填写用于接收通知的 HTTPS 地址；其来源地址需由部署者加入白名单。</small>
        </label>

        <template v-if="channelKind === 'bark'">
          <label class="toggle-line">
            <input v-model="barkEncryptionEnabled" type="checkbox" />
            <span><strong>加密通知内容</strong><small>保护通知标题和正文。</small></span>
          </label>
          <label v-if="barkEncryptionEnabled">
            Bark 加密密钥
            <div class="secret-input-row">
              <input
                v-model="barkEncryptionKey"
                required
                type="password"
                minlength="32"
                maxlength="32"
                autocomplete="new-password"
                placeholder="32 位字符"
              />
              <button type="button" class="secondary" @click="generateBarkEncryptionKey">生成并复制</button>
            </div>
            <small>在 Bark 中选择 AES256、GCM、noPadding，并填写相同密钥。IV 可填写任意 12 位字符。</small>
          </label>
        </template>

        <label v-else>
          Bearer Token（可选）
          <input
            v-model="bearerToken"
            type="password"
            maxlength="2048"
            autocomplete="new-password"
            placeholder="只填写令牌，不包含 Bearer 前缀"
          />
          <small>保存后不会再次显示令牌内容。</small>
        </label>

        <p v-if="channel?.configured" class="credential-note">修改时请重新填写完整配置。</p>
        <div class="notification-actions">
          <button v-if="channel?.configured" type="button" class="danger ghost" :disabled="notificationBusy" @click="deleteNotificationChannel"><Trash2 :size="15" /> 删除配置</button>
          <span v-if="notificationState" class="save-state" aria-live="polite">{{ notificationState }}</span>
          <button v-if="channel?.configured" type="button" class="secondary" :disabled="notificationBusy" @click="testNotificationChannel"><FlaskConical :size="15" /> 测试通知</button>
          <button class="primary" :disabled="notificationBusy"><Save :size="15" /> {{ notificationBusy ? '处理中…' : '保存' }}</button>
        </div>
      </form>
    </section>

    <section class="settings-card danger-zone"><div><h2>退出登录</h2><p>退出当前账号。</p></div><button class="quiet" @click="logout"><LogOut :size="16" /> 退出</button></section>
  </div>
</template>
