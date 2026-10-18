<script lang="ts">
  import { byId } from './lib/commands';
  import { app, clock } from './lib/state.svelte';

  function hint(id: string): [string, string] {
    const c = byId(id);
    return [c?.shortcut ?? '', c?.label ?? id];
  }

  const hints = $derived.by((): [string, string][] => {
    if (app.screen === 'compose') {
      if (app.confirmDiscard) return [['esc', 'di nuovo per scartare']];
      const list = [hint('send'), hint('attach'), hint('bcc')];
      if (app.draft?.quote) list.push(['⌃q', app.draftQuote ? 'nascondi citazione' : 'mostra citazione']);
      list.push(hint('cancel'));
      return list;
    }
    if (app.screen === 'setup') return [['⏎', 'conferma'], ...(app.accounts.length ? [['esc', 'annulla'] as [string, string]] : [])];
    if (app.focus === 'search') return [['⏎', 'conferma'], ['esc', 'annulla']];
    return [['j/k', 'naviga'], ['⏎', 'apri'], hint('archive'), hint('reply'), hint('compose'), hint('search'), [':', 'comandi']];
  });

  const sync = $derived(app.status[app.account]);
</script>

<footer>
  <div class="left">
    {#if app.flash}
      <span class:error={app.flash.error} class="ellipsis">{app.flash.text}</span>
    {:else}
      {#each hints as [key, label], i (i)}
        {#if i > 0}<span class="sep">·</span>{/if}
        <span class="hint"><b>{key}</b> {label}</span>
      {/each}
    {/if}
  </div>
  <div class="right">
    {#if app.screen === 'compose'}
      <span>da {app.draft?.account}</span>
    {:else if app.account}
      {#if !sync || sync.state === 'syncing'}
        <span>sync <span class="dot"></span></span>
      {:else if sync.state === 'error'}
        <span title={sync.message}>errore <span class="dot red"></span></span>
      {:else}
        <span>sync {clock(sync.at)} <span class="dot green"></span></span>
      {/if}
    {/if}
  </div>
</footer>

<style>
  footer {
    height: 32px;
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 0 16px;
    border-top: var(--line);
    font-size: var(--fs-small);
    color: var(--muted);
  }

  .left {
    display: flex;
    gap: 8px;
    min-width: 0;
    overflow: hidden;
  }

  .right {
    flex: none;
  }

  .error {
    color: var(--red);
  }

  .dot {
    color: var(--muted);
  }

  .dot::before {
    content: '●';
  }

  .dot.green {
    color: var(--green);
  }

  .dot.red {
    color: var(--red);
  }
</style>
