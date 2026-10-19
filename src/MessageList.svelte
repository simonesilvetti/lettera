<script lang="ts">
  import { untrack } from 'svelte';
  import { app, ensureLoaded, openThread, shortDate } from './lib/state.svelte';
  import { rowAt, rowCount, rowOf, scrollInto, visibleWindow } from './lib/virtual';

  const ROW = 38;
  let el = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(600);

  const count = $derived(rowCount(app.groups));
  const win = $derived(visibleWindow(scrollTop, height, ROW, count));
  const rows = $derived(
    Array.from({ length: win.end - win.start }, (_, k) => ({
      i: win.start + k,
      row: rowAt(app.groups, win.start + k),
    })),
  );

  $effect(() => {
    const idx = rows.flatMap((r) => (r.row?.kind === 'thread' ? [r.row.index] : []));
    if (idx.length) untrack(() => ensureLoaded(Math.min(...idx), Math.max(...idx) + 1));
  });

  $effect(() => {
    const target = rowOf(app.groups, app.selected);
    if (!el) return;
    // the header above the first row stays visible too
    const top = scrollInto(target === 1 ? 0 : target, el.scrollTop, height, ROW);
    if (top !== null) el.scrollTop = top;
  });
</script>

<div class="list" bind:this={el} bind:clientHeight={height} onscroll={() => (scrollTop = el?.scrollTop ?? 0)}>
  {#if app.loaded && app.total === 0}
    <p class="empty muted">{app.query ? 'no results' : 'no messages'}</p>
  {:else if !app.folderId}
    <p class="empty muted">syncing…</p>
  {/if}
  <div class="spacer" style:height="{win.height}px">
    <div style:transform="translateY({win.top}px)">
      {#each rows as r (r.i)}
        {#if r.row?.kind === 'header'}
          <div class="group">{r.row.label}</div>
        {:else if r.row}
          {@const index = r.row.index}
          {@const t = app.items[index]}
          <button
            class="row"
            class:selected={index === app.selected}
            class:unread={t?.unread}
            onclick={() => openThread(index)}
          >
            <span class="dot"></span>
            {#if t}
              <span class="from ellipsis">{t.from}{t.count > 1 ? ` ${t.count}` : ''}</span>
              <span class="subject ellipsis"
                ><span class="to-me" title={t.to_me ? 'sent to you' : undefined}>{t.to_me ? '›' : ''}</span>{t.flagged ? '* ' : ''}{t.subject || '(no subject)'}</span
              >
              {#if t.has_attachments}
                <svg class="clip" viewBox="0 0 24 24" aria-label="attachment" role="img">
                  <path
                    d="M21.44 11.05l-9.19 9.19a6 6 0 0 1-8.49-8.49l9.19-9.19a4 4 0 0 1 5.66 5.66l-9.2 9.19a2 2 0 0 1-2.83-2.83l8.49-8.48"
                  />
                </svg>
              {/if}
              <span class="date">{shortDate(t.date)}</span>
            {/if}
          </button>
        {/if}
      {/each}
    </div>
  </div>
</div>

<style>
  .list {
    width: 420px;
    flex: none;
    border-right: var(--line);
    overflow-y: auto;
    position: relative;
  }

  .spacer {
    position: relative;
  }

  .empty {
    position: absolute;
    top: 40%;
    width: 100%;
    text-align: center;
  }

  .group {
    height: 38px;
    display: flex;
    align-items: flex-end;
    padding: 0 16px 8px;
    font-size: var(--fs-tiny);
    font-variant: small-caps;
    text-transform: lowercase;
    letter-spacing: 0.12em;
    color: var(--muted);
  }

  .row {
    height: 38px;
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 16px 0 12px;
    color: var(--muted);
  }

  .row.unread {
    color: var(--fg);
    font-weight: bold;
  }

  .row.selected {
    background: var(--surface);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .dot {
    width: 6px;
    height: 6px;
    flex: none;
    background: transparent;
  }

  .unread .dot {
    background: var(--accent);
  }

  /* always takes its column, so subjects line up */
  .to-me {
    display: inline-block;
    width: 1.5ch;
    color: var(--muted);
    font-weight: normal;
  }

  .clip {
    width: 12px;
    height: 12px;
    flex: none;
    fill: none;
    stroke: var(--muted);
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .from {
    width: 150px;
    flex: none;
  }

  .subject {
    flex: 1;
    min-width: 0;
  }

  .date {
    flex: none;
    font-size: var(--fs-small);
    font-weight: normal;
    color: var(--muted);
  }
</style>
