<script setup lang="ts">
import { ArrowUpRight } from '@lucide/vue'
</script>

<template>
  <div class="page narrow-page guide-page">
    <header class="page-header">
      <div><p class="eyebrow">HOW TO USE</p><h1>使用说明</h1><p>先理解规则，再开始使用。</p></div>
    </header>

    <section class="principle-note guide-source">
      <strong>理论来源</strong>
      <span>
        Lumeno 根据 Edmond 在知乎回答
        <a href="https://www.zhihu.com/question/19888447/answer/1930799480401293785" target="_blank" rel="noopener noreferrer">《如何提高自制力？》<ArrowUpRight :size="13" /></a>
        中提出的 CTDP 与 RSIP 设计。这里仅说明软件如何对应原文方法。
      </span>
    </section>

    <section class="paper-panel">
      <h2>这套方法在做什么</h2>
      <p>原文认为，人会给眼前的感受更高权重，所以明知长期有益，也可能难以开始或坚持。CTDP 不依赖反复说服自己，而是把当前选择与已经积累的记录、未来必须一致的规则绑定；RSIP 则把干预提前到仍有能力选择的位置。</p>
    </section>

    <section class="guide-section">
      <header><p class="eyebrow">CTDP</p><h2>用契约链完成一次任务</h2><p>契约链对应原文的链式时延协议，处理启动困难、中途放弃和规则逐渐失效。</p></header>
      <div class="guide-cards">
        <article class="guide-card">
          <h3><span>1</span>写下明确的启动条件</h3>
          <p>原文把它称为“神圣座位”：它可以是一张椅子、一件物品、一个动作或一条消息。未触发时可以不开始；一旦触发，就按约定完成这次专注。</p>
        </article>
        <article class="guide-card">
          <h3><span>2</span>用预约降低启动阻力</h3>
          <p>预约对应“线性时延”。先定义一个简单的预约信号和等待时间；信号一旦触发，就必须在规定时间内启动主任务。</p>
        </article>
        <article class="guide-card">
          <h3><span>3</span>每次例外都成为规则</h3>
          <p>原文称之为“下必为例”。遇到疑似违规时，只能判定失败并让连续记录归零，或把它设为例外；设为例外后，今后相同情况也必须允许。</p>
        </article>
      </div>
    </section>

    <section class="guide-section paper-panel">
      <h2>在 Lumeno 中这样操作</h2>
      <ol class="guide-steps">
        <li>在“契约链”中新建叶子任务，填写启动条件、专注时长、预约信号和预约等待时间。</li>
        <li>条件已经出现时直接开始；暂时难以启动时，先触发预约，稍后按时开始。</li>
        <li>完成约定时长后确认完成，主链连续增加。按时兑现预约只增加预约连续，任务仍需真正完成。</li>
        <li>中断、提前结束或预约未完成时，如实选择失败或例外规则，不使用“只允许这一次”的临时解释。</li>
        <li>需要连续完成多个任务时，用任务群按顺序组织叶子任务。</li>
      </ol>
    </section>

    <section class="guide-section">
      <header><p class="eyebrow">RSIP</p><h2>用定式改变重复出现的状态</h2><p>原文指出，CTDP 适合干预单次行为，却不能独自解决作息、精力、环境和习惯形成的长期稳态。RSIP 用更早、更小的干预逐步改变这些重复模式。</p></header>
      <div class="guide-cards">
        <article class="guide-card">
          <h3><span>1</span>识别重复出现的问题</h3>
          <p>不要只盯着已经难以退出的状态。记录它发生时的表现、此前信号和造成的影响。</p>
        </article>
        <article class="guide-card">
          <h3><span>2</span>向前回溯</h3>
          <p>从结果向前检查每一步，找到一个更早的节点：在那里，两种选择的差距还没有大到无法干预。</p>
        </article>
        <article class="guide-card">
          <h3><span>3</span>建立一个小而明确的定式</h3>
          <p>为这个节点写下可识别的触发条件和必须执行的动作。先在现实中完整试行；当天成功后，才可把当前修订点亮到树中，每天最多点亮一个。</p>
        </article>
        <article class="guide-card">
          <h3><span>4</span>失败时回退</h3>
          <p>某个部署失效时，如实报告。它和依赖它的下游会熄灭，但国策定义与旧修订仍保留，之后可以重新试行和部署。</p>
        </article>
        <article class="guide-card">
          <h3><span>5</span>需要容错时使用国策组</h3>
          <p>组成员只共同决定组是否点亮，下游依赖整个组。成员减少但仍达门槛时下游保留；跌破门槛时下游熄灭，其他成员不会被一并清除。</p>
        </article>
      </div>
    </section>

    <section class="paper-panel guide-boundary">
      <h2>使用边界</h2>
      <p>原文强调，这些方法只能帮助部分人解决部分行为问题，不是衡量个人价值的标准，也不是医疗手段。严重的注意力、情绪或其他身心问题应寻求专业帮助；有时真正需要的也可能是休息，而不是更多约束。</p>
    </section>
  </div>
</template>
