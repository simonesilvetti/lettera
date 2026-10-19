<script lang="ts">
  import wordmark from '../assets/wordmark.svg?raw';
  import Logo from './Logo.svelte';
  import { showInfo } from './lib/state.svelte';
</script>

<!-- Icon and wordmark in the top bar.
     Click: the :info page. -->
<button class="brand" onclick={showInfo} title="lettera · info">
  <Logo size={18} />
  <span class="word">{@html wordmark}</span>
</button>

<style>
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--fg);
  }

  .word :global(svg) {
    display: block;
    height: 14px;
    width: auto;
  }

  .brand:hover .word {
    color: var(--accent);
  }
</style>
