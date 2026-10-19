<script lang="ts">
  // Left column: the four usual folders as icons (Tabler, outline). On hover
  // they tilt 6° and take the accent, like the links on simonesilvetti.com.
  import { byId, runCommand } from './lib/commands';
  import { app, currentFolder } from './lib/state.svelte';

  const ITEMS = [
    {
      role: 'inbox',
      command: 'go-inbox',
      paths: [
        'M4 4m0 2a2 2 0 0 1 2 -2h12a2 2 0 0 1 2 2v12a2 2 0 0 1 -2 2h-12a2 2 0 0 1 -2 -2z',
        'M4 13h3l3 3h4l3 -3h3',
      ],
    },
    {
      role: 'drafts',
      command: 'go-drafts',
      paths: [
        'M14 3v4a1 1 0 0 0 1 1h4',
        'M17 21h-10a2 2 0 0 1 -2 -2v-14a2 2 0 0 1 2 -2h7l5 5v11a2 2 0 0 1 -2 2z',
        'M10 18l5 -5a1.414 1.414 0 0 0 -2 -2l-5 5v2h2z',
      ],
    },
    {
      role: 'sent',
      command: 'go-sent',
      paths: ['M10 14l11 -11', 'M21 3l-6.5 18a.55 .55 0 0 1 -1 0l-3.5 -7l-7 -3.5a.55 .55 0 0 1 0 -1l18 -6.5'],
    },
    {
      role: 'archive',
      command: 'go-archive',
      paths: [
        'M3 4m0 2a2 2 0 0 1 2 -2h14a2 2 0 0 1 2 2v0a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2z',
        'M5 8v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2 -2v-10',
        'M10 12l4 0',
      ],
    },
  ];

  // archive falls back to "all mail" (Gmail), as goToRole does
  const role = $derived.by(() => {
    const f = currentFolder();
    if (f?.role === 'all' && !app.folders.some((x) => x.role === 'archive')) return 'archive';
    return f?.role ?? '';
  });
</script>

<nav>
  {#each ITEMS as item (item.role)}
    {@const c = byId(item.command)}
    <button
      class:current={role === item.role}
      title="{item.role} · {c?.shortcut ?? ''}"
      aria-label={item.role}
      onclick={() => runCommand(item.command)}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        {#each item.paths as d (d)}
          <path {d} />
        {/each}
      </svg>
    </button>
  {/each}
</nav>

<style>
  nav {
    width: 44px;
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding-top: 8px;
    border-right: var(--line);
  }

  button {
    display: block;
    padding: 6px;
    color: var(--muted);
  }

  button.current {
    color: var(--fg);
  }

  button:hover {
    rotate: 6deg;
    color: var(--accent);
  }

  svg {
    display: block;
    width: 20px;
    height: 20px;
  }
</style>
