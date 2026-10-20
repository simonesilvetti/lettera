<script lang="ts">
  import { api } from './lib/api';
  import { commands } from './lib/commands';
  import { fuzzy, highlight } from './lib/fuzzy';
  import { available } from './lib/keys';
  import { app, closePalette, type PaletteItem } from './lib/state.svelte';
  import wordmark from '../assets/wordmark.svg?raw';

  const p = $derived(app.palette!);
  let input = $state<HTMLInputElement>();
  let list = $state<HTMLUListElement>();

  /** Scopes as if the palette were closed. */
  function scopesBehind() {
    if (app.screen === 'compose') return ['compose', 'global'] as const;
    if (app.screen === 'setup') return ['global'] as const;
    return app.thread ? (['thread', 'mail', 'global'] as const) : (['list', 'mail', 'global'] as const);
  }

  const items = $derived.by((): PaletteItem[] => {
    if (p.items) return p.items;
    const scopes = [...scopesBehind()];
    return commands
      .filter((c) => !c.hidden && available(c, scopes))
      .map((c) => ({ label: c.label, hint: c.shortcut, alias: c.alias, run: c.run }));
  });

  const matches = $derived(
    items
      .map((item) => {
        const m = fuzzy(p.query, item.label);
        const a = !m && item.alias ? fuzzy(p.query, item.alias) : null;
        const best = m ?? (a ? { score: a.score - 1, positions: [] } : null);
        return best ? { item, ...best } : null;
      })
      .filter((x) => x !== null)
      .sort((a, b) => (p.query ? b.score - a.score : 0)),
  );

  $effect(() => {
    input?.focus();
  });

  $effect(() => {
    const row = list?.children[p.index] as HTMLElement | undefined;
    row?.scrollIntoView({ block: 'nearest' });
  });

  function choose(i: number) {
    const m = matches[i];
    if (!m) return;
    closePalette();
    void m.item.run();
  }

  function onkey(e: KeyboardEvent) {
    const n = matches.length;
    if (e.key === 'ArrowDown' || (e.ctrlKey && e.key === 'n')) {
      e.preventDefault();
      p.index = n ? (p.index + 1) % n : 0;
    } else if (e.key === 'ArrowUp' || (e.ctrlKey && e.key === 'p')) {
      e.preventDefault();
      p.index = n ? (p.index - 1 + n) % n : 0;
    } else if (e.key === 'Enter') {
      e.preventDefault();
      choose(p.index);
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="overlay" onclick={closePalette}>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="palette" onclick={(e) => e.stopPropagation()}>
    {#if p.info}
      <div class="info">
        <span class="wordmark">{@html wordmark}</span>
        <div class="about">
          <p class="muted">{app.info?.name} {app.info?.version} · license {app.info?.license} · {app.info?.config}</p>
          {#if app.info}
            <p class="muted">
              by {app.info.author} ·
              <a href={app.info.homepage} onclick={(e) => (e.preventDefault(), api.openUrl(app.info!.homepage))}
                >{app.info.homepage.replace(/^https?:\/\//, '')}</a
              >
            </p>
          {/if}
        </div>
      </div>
      <input class="sr" bind:this={input} aria-label="close" />
      <div class="hints muted">esc close</div>
    {:else}
      <label class="prompt">
        <span class="accent">›</span>
        <input
          bind:this={input}
          bind:value={p.query}
          placeholder={p.title || 'command'}
          oninput={() => (p.index = 0)}
          onkeydown={onkey}
        />
      </label>
      <ul bind:this={list}>
        {#each matches as m, i (m.item.label + i)}
          <li>
            <button class:selected={i === p.index} onclick={() => choose(i)} onmousemove={() => (p.index = i)}>
              <span class="label">
                {#each highlight(m.item.label, m.positions) as [chunk, hit], k (k)}
                  <span class:hit>{chunk}</span>
                {/each}
              </span>
              {#if m.item.hint}<span class="muted small">{m.item.hint}</span>{/if}
            </button>
          </li>
        {:else}
          <li class="none muted">no commands</li>
        {/each}
      </ul>
      <div class="hints muted">↑↓ choose · ⏎ run · esc close</div>
    {/if}
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    animation: fade 80ms ease-out;
  }

  .palette {
    position: absolute;
    top: 112px;
    left: 50%;
    width: 600px;
    max-width: calc(100% - 32px);
    transform: translateX(-50%);
    background: var(--bg);
    border: var(--line);
  }

  .prompt {
    display: flex;
    gap: 10px;
    align-items: center;
    height: 44px;
    padding: 0 16px;
    border-bottom: var(--line);
  }

  .prompt input {
    flex: 1;
  }

  .accent,
  .hit {
    color: var(--accent);
  }

  ul {
    list-style: none;
    max-height: 380px;
    overflow-y: auto;
  }

  li button {
    width: 100%;
    height: 34px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 0 16px;
  }

  li button.selected {
    background: var(--surface);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .label {
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .none {
    padding: 10px 16px;
  }

  .hints {
    padding: 8px 16px;
    border-top: var(--line);
    font-size: var(--fs-small);
  }

  .info {
    padding: 28px 24px 22px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .about {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .about p {
    margin: 0;
  }

  .wordmark :global(svg) {
    height: 28px;
    width: auto;
    display: block;
    color: var(--fg);
  }

  .sr {
    position: absolute;
    opacity: 0;
    width: 1px;
    height: 1px;
  }
</style>
