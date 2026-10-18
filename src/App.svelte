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
            placeholder="cerca in {folderLabel(currentFolder())}"
            oninput={(e) => setQuery(e.currentTarget.value)}
            onfocus={() => (app.focus = 'search')}
          />
          <span class="muted small">{app.total} risultati</span>
        </label>
      {:else}
        <div class="title">
          <strong>{folderLabel(currentFolder()) || 'lettera'}</strong>
          <span class="muted">{app.unread} non letti · {app.total}</span>
        </div>
      {/if}
      <span class="muted ellipsis">{app.account}</span>
    </header>
    <main>
      <MessageList />
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
  }
</style>
