<script lang="ts">
  import { onMount } from 'svelte';
  import AccountForm from './AccountForm.svelte';
  import Compose from './Compose.svelte';
  import MessageList from './MessageList.svelte';
  import Palette from './Palette.svelte';
  import StatusLine from './StatusLine.svelte';
  import Thread from './Thread.svelte';
  import { handleKey } from './lib/keys';
  import { app, currentFolder, flash, folderLabel, setQuery, start } from './lib/state.svelte';

  let search = $state<HTMLInputElement>();

  // ------------------------------------------------ resizable list pane
  const DEFAULT_LIST = 420;
  const MIN_LIST = 300;
  const MIN_THREAD = 360;
  const KEY = 'lettera.listWidth';
  let mainWidth = $state(0);
  let saved = DEFAULT_LIST;
  try {
    saved = Number(localStorage.getItem(KEY)) || DEFAULT_LIST;
  } catch {
    // storage unavailable: keep the default
  }
  let wanted = $state(saved);
  let dragging = $state(false);
  // the window may shrink below the saved width: clamp without forgetting it
  const listWidth = $derived(
    Math.round(Math.max(MIN_LIST, Math.min(wanted, (mainWidth || Infinity) - MIN_THREAD))),
  );

  function setListWidth(w: number) {
    wanted = w;
    try {
      localStorage.setItem(KEY, String(Math.round(w)));
    } catch {
      // not persisted, still applied
    }
  }

  function startDrag(e: PointerEvent) {
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    try {
      el.setPointerCapture(e.pointerId);
    } catch {
      // no capture (synthetic events): moves still arrive while over the bar
    }
    const left = el.parentElement!.getBoundingClientRect().left;
    dragging = true;
    const move = (ev: PointerEvent) => (wanted = Math.max(MIN_LIST, Math.min(ev.clientX - left, mainWidth - MIN_THREAD)));
    const up = () => {
      dragging = false;
      setListWidth(listWidth);
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      el.removeEventListener('pointercancel', up);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
    el.addEventListener('pointercancel', up);
  }

  onMount(() => {
    start().catch((e) => flash(String(e), true));
  });

  $effect(() => {
    if (app.focus === 'search') search?.focus();
    else search?.blur();
  });
</script>

<svelte:window onkeydown={handleKey} />

<div class="app" class:dim={!!app.palette}>
  {#if app.screen === 'compose'}
    <Compose />
  {:else if app.screen === 'setup'}
    <AccountForm />
    <StatusLine />
  {:else if app.screen === 'inbox'}
    <header>
      {#if app.focus === 'search' || app.query}
        <label class="search">
          <span class="accent">/</span>
          <input
            bind:this={search}
            value={app.query}
            placeholder="search {folderLabel(currentFolder())}"
            oninput={(e) => setQuery(e.currentTarget.value)}
            onfocus={() => (app.focus = 'search')}
          />
          <span class="muted small">{app.total} results</span>
        </label>
      {:else}
        <div class="title">
          <strong>{folderLabel(currentFolder()) || 'lettera'}</strong>
          <span class="muted">{app.unread} unread · {app.total}</span>
        </div>
      {/if}
      <span class="muted ellipsis">{app.account}</span>
    </header>
    <main bind:clientWidth={mainWidth} style:--list-width="{listWidth}px">
      <MessageList />
      <!-- drag to resize the list; double-click restores the default width -->
      <div
        class="splitter"
        class:dragging
        role="separator"
        aria-orientation="vertical"
        aria-valuenow={listWidth}
        tabindex="-1"
        onpointerdown={startDrag}
        ondblclick={() => setListWidth(DEFAULT_LIST)}
      ></div>
      <Thread />
    </main>
    <StatusLine />
  {/if}
</div>

{#if app.palette}
  <Palette />
{/if}

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
    transition: opacity 80ms;
  }

  .dim {
    opacity: 0.35;
  }

  header {
    height: 40px;
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 0 16px;
    border-bottom: var(--line);
  }

  .title {
    display: flex;
    gap: 12px;
    align-items: baseline;
    white-space: nowrap;
  }

  .search {
    display: flex;
    gap: 10px;
    align-items: center;
    flex: 1;
    max-width: 600px;
  }

  .search input {
    flex: 1;
  }

  .accent {
    color: var(--accent);
  }

  main {
    flex: 1;
    display: flex;
    min-height: 0;
    position: relative;
  }

  /* invisible 7px grab area centred on the list's 1px border */
  .splitter {
    flex: none;
    width: 7px;
    margin: 0 -4px 0 -3px;
    z-index: 1;
    cursor: col-resize;
    touch-action: none;
  }

  .splitter:hover,
  .splitter.dragging {
    background: linear-gradient(var(--accent), var(--accent)) center / 1px 100% no-repeat;
  }
</style>
