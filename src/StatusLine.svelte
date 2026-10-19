<script lang="ts">
  import { byId, runCommand } from './lib/commands';
  import { app, clock } from './lib/state.svelte';

  /** A key hint; with `run` it is also a button. */
  interface Hint {
    key: string;
    label: string;
    run?: () => unknown;
    off?: boolean;
  }

  function hint(id: string, label?: string, key?: string): Hint {
    const c = byId(id);
    return {
      key: key ?? c?.shortcut ?? '',
      label: label ?? c?.label ?? id,
      run: () => runCommand(id),
      off: c?.when ? !c.when() : false,
    };
  }

  function closeSetup() {
    app.setupOpen = false;
    app.screen = 'inbox';
  }

  const hints = $derived.by((): Hint[] => {
    if (app.screen === 'compose') {
      if (app.confirmDiscard) return [hint('cancel', 'again to discard')];
      const list = [hint('send'), hint('attach')];
      list.push(hint('bcc'));
      if (app.draft?.quote) list.push(hint('quote', app.draftQuote ? 'hide quote' : 'show quote'));
      list.push(hint('cancel'));
      return list;
    }
    if (app.screen === 'setup') {
      const submit = () => document.querySelector<HTMLFormElement>('.setup form')?.requestSubmit();
      const list: Hint[] = [{ key: '⏎', label: 'confirm', run: submit }];
      if (app.accounts.length) list.push({ key: 'esc', label: 'cancel', run: closeSetup });
      return list;
    }
    if (app.focus === 'search') return [hint('search-done', 'confirm', '⏎'), hint('search-cancel', 'cancel', 'esc')];
    return [
      { key: 'j/k', label: 'navigate' },
      hint('open', 'open'),
      hint('archive'),
      hint('reply'),
      hint('compose'),
      hint('search'),
      hint('palette'),
    ];
  });

  const sync = $derived(app.status[app.account]);
</script>

<footer>
  <div class="left">
    {#if app.flash}
      <span class:error={app.flash.error} class="ellipsis">{app.flash.text}</span>
    {:else}
      {#each hints as h, i (i)}
        {#if i > 0}<span class="sep">·</span>{/if}
        {#if h.run}
          <button class="hint" disabled={h.off} onclick={h.run}><b>{h.key}</b> {h.label}</button>
        {:else}
          <span class="hint"><b>{h.key}</b> {h.label}</span>
        {/if}
      {/each}
    {/if}
  </div>
  <div class="right">
    {#if app.screen === 'compose'}
      <span>from {app.draft?.account}</span>
    {:else if app.account}
      {#if !sync || sync.state === 'syncing'}
        <span>sync <span class="dot"></span></span>
      {:else if sync.state === 'error'}
        <span title={sync.message}>error <span class="dot red"></span></span>
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
