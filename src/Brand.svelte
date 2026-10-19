<script lang="ts">
  import wordmark from '../assets/wordmark.svg?raw';
  import { showInfo } from './lib/state.svelte';
</script>

<!-- Icon and wordmark in the top bar, drawn with the theme's colors.
     Click: the :info page. -->
<button class="brand" onclick={showInfo} title="lettera · info">
  <svg class="icon" viewBox="160 160 704 680" aria-hidden="true">
    <defs>
      <mask id="brand-keys" maskUnits="userSpaceOnUse" x="0" y="0" width="1024" height="1024">
        <rect width="1024" height="1024" fill="#fff" />
        <g fill="#000">
          <circle cx="272" cy="610" r="24" /><circle cx="352" cy="610" r="24" /><circle cx="432" cy="610" r="24" /><circle
            cx="512"
            cy="610"
            r="24"
          /><circle cx="592" cy="610" r="24" /><circle cx="672" cy="610" r="24" /><circle cx="752" cy="610" r="24" />
          <circle cx="312" cy="690" r="24" /><circle cx="392" cy="690" r="24" /><circle cx="472" cy="690" r="24" /><circle
            cx="552"
            cy="690"
            r="24"
          /><circle cx="632" cy="690" r="24" /><circle cx="712" cy="690" r="24" />
          <rect x="392" y="748" width="240" height="32" />
        </g>
      </mask>
      <mask id="brand-flap" maskUnits="userSpaceOnUse" x="0" y="0" width="1024" height="1024">
        <rect width="1024" height="1024" fill="#fff" />
        <polyline points="312,192 512,340 712,192" fill="none" stroke="#000" stroke-width="40" />
      </mask>
    </defs>
    <rect class="letter" x="292" y="176" width="440" height="284" mask="url(#brand-flap)" />
    <rect class="machine" x="232" y="440" width="560" height="56" />
    <rect class="machine" x="172" y="528" width="680" height="296" mask="url(#brand-keys)" />
  </svg>
  <span class="word">{@html wordmark}</span>
</button>

<style>
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--fg);
  }

  .icon {
    width: 22px;
    height: 22px;
    flex: none;
  }

  .letter {
    fill: var(--accent);
  }

  .machine {
    fill: var(--fg);
  }

  .word :global(svg) {
    display: block;
    height: 17px;
    width: auto;
  }

  .brand:hover .word {
    color: var(--accent);
  }
</style>
