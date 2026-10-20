<script lang="ts">
  import { onMount } from 'svelte';
  import StatusLine from './StatusLine.svelte';
  import { api } from './lib/api';
  import { runCommand } from './lib/commands';
  import { app, clock, fileName, removeAttachment, touchDraft } from './lib/state.svelte';
  import type { Addr } from './lib/types';

  type Field = 'to' | 'cc' | 'bcc';

  const d = $derived(app.draft!);
  let suggestions = $state<Addr[]>([]);
  let active = $state<Field | null>(null);
  let pick = $state(0);
  let first = $state<HTMLInputElement>();
  let bodyEl = $state<HTMLTextAreaElement>();

  const title = $derived.by(() => {
    if (d.kind === 'new') return 'new message';
    if (d.kind === 'forward') return `forward: ${d.subject.replace(/^fwd:\s*/i, '')}`;
    const to = d.to.split(',')[0]?.replace(/<.*>/, '').trim();
    return `reply to ${to || '…'}`;
  });

  onMount(() => {
    // the caret starts above the signature, whenever the body gets focus
    bodyEl?.setSelectionRange(0, 0);
    if (d.kind === 'new' || d.kind === 'forward' || !d.to) first?.focus();
    else bodyEl?.focus();
  });

  let timer: ReturnType<typeof setTimeout> | undefined;
  function lookup(field: Field, value: string) {
    active = field;
    pick = 0;
    const token = value.split(/[,;]/).pop()?.trim() ?? '';
    clearTimeout(timer);
    if (token.length < 2) {
      suggestions = [];
      return;
    }
    timer = setTimeout(async () => {
      suggestions = await api.suggestContacts(token);
    }, 80);
  }

  function complete(field: Field, a: Addr) {
    const parts = d[field].split(/[,;]/).map((s) => s.trim()).filter(Boolean);
    parts.pop();
    parts.push(a.name ? `${a.name} <${a.addr}>` : a.addr);
    d[field] = parts.join(', ') + ', ';
    suggestions = [];
    touchDraft();
  }

  function onkey(field: Field, e: KeyboardEvent) {
    if (!suggestions.length || active !== field) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      pick = (pick + 1) % suggestions.length;
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      pick = (pick - 1 + suggestions.length) % suggestions.length;
    } else if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault();
      complete(field, suggestions[pick]);
    }
  }

  function input(field: Field, e: Event & { currentTarget: HTMLInputElement }) {
    d[field] = e.currentTarget.value;
    touchDraft();
    lookup(field, e.currentTarget.value);
  }

  const fields = $derived<[Field, string][]>(app.draftBcc ? [['to', 'to'], ['cc', 'cc'], ['bcc', 'bcc']] : [['to', 'to'], ['cc', 'cc']]);
</script>

<div class="compose">
  <header>
    <span>{title}</span>
    <span class="muted">{app.draftSavedAt ? `draft saved ${clock(app.draftSavedAt)}` : ''}</span>
  </header>

  <div class="scroll">
    {#each fields as [field, label], i (field)}
      <label class="field">
        <span class="muted">{label}</span>
        {#if i === 0}
          <input
            bind:this={first}
            value={d[field]}
            oninput={(e) => input(field, e)}
            onkeydown={(e) => onkey(field, e)}
            onblur={() => setTimeout(() => (suggestions = []), 150)}
          />
        {:else}
          <input
            value={d[field]}
            oninput={(e) => input(field, e)}
            onkeydown={(e) => onkey(field, e)}
            onblur={() => setTimeout(() => (suggestions = []), 150)}
          />
        {/if}
      </label>
      {#if active === field && suggestions.length}
        <ul class="suggest">
          {#each suggestions as s, k (s.addr)}
            <li>
              <button class:selected={k === pick} onmousedown={(e) => (e.preventDefault(), complete(field, s))}>
                {s.name || s.addr} <span class="muted">{s.name ? s.addr : ''}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    {/each}
    <label class="field">
      <span class="muted">subject</span>
      <input bind:value={d.subject} oninput={touchDraft} />
    </label>

    <textarea
      bind:this={bodyEl}
      bind:value={d.body}
      oninput={touchDraft}
      spellcheck="true"
      placeholder="write…"
    ></textarea>

    {#if d.quote}
      {#if app.draftQuote}
        <pre class="quote muted selectable">{d.quote}</pre>
      {:else}
        <button class="hint quote-hint" onclick={() => runCommand('quote')}><b>⌃q</b> show quote</button>
      {/if}
    {/if}

    <div class="files">
      <span class="muted">attachments</span>
      {#if d.attachments.length === 0}
        <button class="hint" onclick={() => runCommand('attach')}>none · <b>⌃⇧a</b> to add</button>
      {:else}
        {#each d.attachments as path, i (path + i)}
          <button class="file" onclick={() => removeAttachment(i)} title="click to remove">
            {fileName(path)}
          </button>
        {/each}
        <button class="hint" onclick={() => runCommand('attach')}><b>⌃⇧a</b> add</button>
      {/if}
    </div>
  </div>

  <StatusLine />
</div>

<style>
  .compose {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  header {
    height: 40px;
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 16px;
    border-bottom: var(--line);
  }

  .scroll {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }

  .field {
    height: 40px;
    flex: none;
    display: flex;
    align-items: center;
    padding: 0 16px;
    border-bottom: var(--line);
  }

  .field span {
    width: 72px;
    flex: none;
  }

  .field input {
    flex: 1;
    height: 100%;
  }

  .suggest {
    list-style: none;
    border-bottom: var(--line);
    padding-left: 88px;
  }

  .suggest button {
    width: 100%;
    height: 30px;
    padding: 0 8px;
  }

  .suggest button.selected {
    background: var(--surface);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  textarea {
    flex: 1;
    min-height: 240px;
    width: 100%;
    max-width: calc(72ch + 32px);
    padding: 18px 16px;
    resize: none;
    font-size: var(--fs-body);
    line-height: 1.65;
  }

  .quote,
  .quote-hint {
    padding: 0 16px 16px;
    max-width: calc(72ch + 32px);
    font-family: inherit;
    font-size: var(--fs-body);
    line-height: 1.65;
    white-space: pre-wrap;
  }

  .quote-hint {
    font-size: var(--fs-small);
  }

  .files {
    flex: none;
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    padding: 10px 16px;
    border-top: var(--line);
  }

  .file:hover {
    color: var(--red);
    text-decoration: line-through;
  }
</style>
