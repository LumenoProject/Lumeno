import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { VitePWA } from 'vite-plugin-pwa'

export default defineConfig({
  plugins: [
    vue(),
    VitePWA({
      strategies: 'injectManifest',
      srcDir: 'src',
      filename: 'sw.ts',
      registerType: 'autoUpdate',
      includeManifestIcons: false,
      manifest: {
        name: 'Lumeno',
        short_name: 'Lumeno',
        description: '基于 CTDP 与 RSIP 的严格自控协议工具',
        theme_color: '#F5F2EA',
        background_color: '#F5F2EA',
        display: 'standalone',
        start_url: '/',
        id: '/',
        lang: 'zh-CN',
        icons: [
          { src: '/pwa-icon-192.png', sizes: '192x192', type: 'image/png', purpose: 'any' },
          { src: '/pwa-icon-512.png', sizes: '512x512', type: 'image/png', purpose: 'any' },
          { src: '/pwa-icon-maskable-512.png', sizes: '512x512', type: 'image/png', purpose: 'maskable' },
        ],
      },
      injectManifest: {
        globPatterns: ['**/*.{js,css,html,png,woff2}'],
        rollupFormat: 'iife',
      },
    }),
  ],
  server: {
    proxy: { '/api': 'http://127.0.0.1:3000' },
  },
  build: { target: 'es2023' },
})
