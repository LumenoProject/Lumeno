<script setup lang="ts">
import { onMounted } from 'vue'
import { RouterLink, RouterView } from 'vue-router'
import { BookOpenText, History, ListTree, Settings, Sparkles, Target, WifiOff } from '@lucide/vue'
import FocusOverlay from './components/FocusOverlay.vue'
import GroupJudgment from './components/GroupJudgment.vue'
import BookingJudgment from './components/BookingJudgment.vue'
import BookingOverlay from './components/BookingOverlay.vue'
import CommitmentGraceOverlay from './components/CommitmentGraceOverlay.vue'
import { useAppStore } from './stores/app'

const app = useAppStore()
onMounted(() => app.bootstrap())

const nav = [
  { to: '/', label: '今日', icon: Sparkles },
  { to: '/chains', label: '契约链', icon: ListTree },
  { to: '/rsip', label: '定式', icon: Target },
  { to: '/history', label: '记录', icon: History },
  { to: '/guide', label: '说明', icon: BookOpenText },
  { to: '/settings', label: '设置', icon: Settings },
]
</script>

<template>
  <div v-if="!app.authReady" class="center-state">
    <img class="brand-mark" src="/icon.png" alt="" />
    <p>正在唤醒 Lumeno…</p>
  </div>

  <main v-else-if="!app.me" class="login-page">
    <section class="login-copy">
      <div class="eyebrow">LUMENO · 行动协议</div>
      <h1>让承诺成为<br><em>可以执行的结构。</em></h1>
      <p>规划任务、保持专注，并回顾每次完成与失守。</p>
      <button class="primary large" @click="app.login">登录 Lumeno</button>
      <small>Lumeno 不会保存你的密码。</small>
    </section>
    <section class="login-card" aria-label="产品方法预览">
      <span class="paper-number">01</span>
      <h2>明确启动条件</h2>
      <p>条件出现时开始行动，计时不会暂停。</p>
      <div class="rule"></div>
      <span class="paper-number">02</span>
      <h2>失败后的回溯</h2>
      <p>找到仍可改变的一步，为下一次制定行动。</p>
    </section>
  </main>

  <div v-else class="shell">
    <aside class="sidebar">
      <RouterLink to="/" class="brand">
        <img class="brand-mark" src="/icon.png" alt="" />
        <span>Lumeno</span>
      </RouterLink>
      <nav>
        <RouterLink v-for="item in nav" :key="item.to" :to="item.to">
          <component :is="item.icon" :size="17" />
          <span>{{ item.label }}</span>
        </RouterLink>
      </nav>
      <div class="sidebar-foot">
        <div v-if="!app.online" class="sync-state warning"><WifiOff :size="14" /> 离线</div>
        <button v-else-if="app.queued" class="sync-state" @click="app.sync">同步 {{ app.queued }} 项</button>
        <div class="sidebar-user" aria-label="当前用户">
          <span class="user-avatar">{{ [...app.me.displayName.trim()][0] || 'L' }}</span>
          <span class="sidebar-user-copy">
            <strong :title="app.me.displayName">{{ app.me.displayName }}</strong>
            <small :title="app.me.email || app.me.timezone">{{ app.me.email || app.me.timezone }}</small>
          </span>
        </div>
      </div>
    </aside>

    <section class="workspace">
      <div v-if="app.error" class="error-banner">
        <span>{{ app.error }}</span><button aria-label="关闭" @click="app.error = ''">×</button>
      </div>
      <RouterView />
    </section>
    <CommitmentGraceOverlay v-if="app.pendingCommitment" />
    <FocusOverlay v-else-if="app.activeFocus" />
    <GroupJudgment v-else-if="app.pendingGroupJudgment" />
    <BookingJudgment v-else-if="app.pendingBookingJudgment" />
    <BookingOverlay v-else-if="app.activeBooking" />
  </div>
</template>
