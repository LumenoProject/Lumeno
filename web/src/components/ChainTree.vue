<script setup lang="ts">
import { computed } from 'vue'
import ChainCard from './ChainCard.vue'
import type { Chain } from '../types'

defineOptions({ name: 'ChainTree' })
const props = defineProps<{ chain: Chain; chains: Chain[]; compact?: boolean }>()
const children = computed(() => props.chains
  .filter((item) => item.parentId === props.chain.id)
  .sort((left, right) => left.position - right.position))
</script>

<template>
  <div class="tree-node">
    <ChainCard :chain="chain" :compact="compact" manage />
    <div v-if="children.length" class="tree-children">
      <ChainTree v-for="child in children" :key="child.id" :chain="child" :chains="chains" compact />
    </div>
  </div>
</template>
