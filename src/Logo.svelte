<script lang="ts">
  // The app icon drawn with the theme's colors: envelope in --accent,
  // typewriter in --fg. Same shape as assets/icon.svg.
  let { size = 18 }: { size?: number } = $props();
  const id = Math.random().toString(36).slice(2);
</script>

<svg class="logo" viewBox="160 160 704 680" width={size} height={size} aria-hidden="true">
  <defs>
    <mask id="keys-{id}" maskUnits="userSpaceOnUse" x="0" y="0" width="1024" height="1024">
      <rect width="1024" height="1024" fill="#fff" />
      <g fill="#000">
        {#each [272, 352, 432, 512, 592, 672, 752] as cx (cx)}<circle {cx} cy="610" r="24" />{/each}
        {#each [312, 392, 472, 552, 632, 712] as cx (cx)}<circle {cx} cy="690" r="24" />{/each}
        <rect x="392" y="748" width="240" height="32" />
      </g>
    </mask>
    <mask id="flap-{id}" maskUnits="userSpaceOnUse" x="0" y="0" width="1024" height="1024">
      <rect width="1024" height="1024" fill="#fff" />
      <polyline points="312,192 512,340 712,192" fill="none" stroke="#000" stroke-width="40" />
    </mask>
  </defs>
  <rect class="letter" x="292" y="176" width="440" height="284" mask="url(#flap-{id})" />
  <rect class="machine" x="232" y="440" width="560" height="56" />
  <rect class="machine" x="172" y="528" width="680" height="296" mask="url(#keys-{id})" />
</svg>

<style>
  .logo {
    flex: none;
    display: block;
  }

  .letter {
    fill: var(--accent);
  }

  .machine {
    fill: var(--fg);
  }
</style>
