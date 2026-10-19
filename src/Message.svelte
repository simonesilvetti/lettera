<script lang="ts">
  import Clip from './Clip.svelte';
  import { api } from './lib/api';
  import { byId } from './lib/commands';
  import {
    app,
    flash,
    longDate,
    saveAttachment,
    shortDate,
    size,
    toggleExpanded,
  } from './lib/state.svelte';
  import type { Addr, MessageView } from './lib/types';

  let { message, index }: { message: MessageView; index: number } = $props();

  let el = $state<HTMLElement>();
  let frame = $state<HTMLIFrameElement>();
  let frameHeight = $state(400);

  const expanded = $derived(!!app.expanded[message.id]);
  const isCursor = $derived(app.focus === 'thread' && app.cursor === index);
  const body = $derived(app.bodies[message.id]);
  const html = $derived(!!app.htmlMode[message.id]);
  const files = $derived(app.attachments[message.id] ?? []);
  // same rule as the list: the account's own address is a To: recipient
  const toMe = $derived(message.to.some((a) => a.addr.toLowerCase() === app.account.toLowerCase()));

  $effect(() => {
    if (isCursor) el?.scrollIntoView({ block: 'nearest' });
  });

  const URL_RE = /(https?:\/\/[^\s<>"'()\]]+[^\s<>"'()\].,;:!?]|mailto:[^\s<>"']+)/g;

  function segments(text: string): [string, boolean][] {
    const out: [string, boolean][] = [];
    let last = 0;
    for (const m of text.matchAll(URL_RE)) {
      if (m.index! > last) out.push([text.slice(last, m.index), false]);
      out.push([m[0], true]);
      last = m.index! + m[0].length;
    }
    if (last < text.length) out.push([text.slice(last), false]);
    return out;
  }

  function open(url: string) {
    api.openUrl(url).catch((e) => flash(String(e), true));
  }

  function people(list: Addr[]) {
    return list.map((a) => a.name || a.addr).join(', ');
  }

  function hookFrame() {
    const doc = frame?.contentDocument;
    if (!doc) return;
    frameHeight = Math.max(120, Math.ceil(doc.body.getBoundingClientRect().height) + 48);
    doc.addEventListener('click', (e) => {
      const a = (e.target as Element | null)?.closest?.('a');
      if (!a) return;
      e.preventDefault();
      const href = a.getAttribute('href');
      if (href) open(href);
    });
  }

  function run(id: string) {
    app.cursor = index;
    void byId(id)?.run();
  }

  function select() {
    app.cursor = index;
    app.focus = 'thread';
    toggleExpanded(message.id);
  }
</script>

<article bind:this={el} class:cursor={isCursor}>
  {#if !expanded}
    <button class="collapsed" onclick={select}>
      <span class="who ellipsis" class:unread={message.unread}>{message.from.name || message.from.addr}</span>
      <span class="snippet ellipsis muted"
        ><span class="to-me" title={toMe ? 'sent to you' : undefined}>{toMe ? '›' : ''}</span>{message.snippet}</span
      >
      {#if message.has_attachments}<Clip />{/if}
      <span class="date muted">{shortDate(message.date)}</span>
    </button>
  {:else}
    <button class="top" onclick={select} title={message.from.addr}>
      <strong class="ellipsis">{message.from.name || message.from.addr}</strong>
      <span class="right">
        {#if message.has_attachments}<Clip />{/if}
        <span class="date muted">{longDate(message.date)}</span>
      </span>
    </button>
    <p class="to muted ellipsis">
      <span class="to-me" title={toMe ? 'sent to you' : undefined}>{toMe ? '›' : ''}</span>to {people(message.to) || '—'}{message.cc.length ? `, cc ${people(message.cc)}` : ''}
    </p>

    {#if !body || !body.ready}
      <p class="muted body">loading…</p>
    {:else if html && body.html}
      <iframe
        bind:this={frame}
        title="original html"
        sandbox="allow-same-origin"
        srcdoc={body.html}
        style:height="{frameHeight}px"
        onload={hookFrame}
      ></iframe>
    {:else}
      <div class="body selectable">
        {#each body.blocks as block, b (b)}
          {#if block.kind === 'quote'}
            {@const key = `${message.id}:${b}`}
            <button class="quote-toggle muted" onclick={() => (app.quotes[key] = !app.quotes[key])}>
              ··· {block.lines} {block.lines === 1 ? 'quoted line' : 'quoted lines'}
            </button>
            {#if app.quotes[key]}
              <div class="quote muted">{block.text}</div>
            {/if}
          {:else}
            <!-- prettier-ignore -->
            <div class="text">{#each segments(block.text) as [chunk, link], s (s)}{#if link}<a href={chunk} onclick={(e) => (e.preventDefault(), open(chunk))}>{chunk}</a>{:else}{chunk}{/if}{/each}</div>
          {/if}
        {/each}
        {#if body.blocks.length === 0}
          <p class="muted">(empty message)</p>
        {/if}
      </div>
    {/if}

    {#if files.length}
      <div class="files">
        {#each files as f (f.id)}
          <button class="file" onclick={() => saveAttachment(f)}>
            <span class="muted">attachment</span> {f.filename} <span class="muted">· {size(f.size)}</span>
          </button>
        {/each}
      </div>
    {/if}

    <div class="actions">
      <button class="hint" onclick={() => run('reply')}><b>r</b> reply</button>
      <span class="muted">·</span>
      <button class="hint" onclick={() => run('reply-all')}><b>a</b> reply all</button>
      <span class="muted">·</span>
      <button class="hint" onclick={() => run('forward')}><b>f</b> forward</button>
      <span class="muted">·</span>
      <button class="hint" onclick={() => run('archive')}><b>e</b> archive</button>
      <span class="muted">·</span>
      <button class="hint" onclick={() => run(html ? 'reading' : 'html')}>
        <b>v</b> {html ? 'reading mode' : 'original html'}
      </button>
    </div>
  {/if}
</article>

<style>
  article {
    border-bottom: var(--line);
    padding: 0 32px;
  }

  article.cursor {
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .collapsed {
    width: 100%;
    height: 40px;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .who {
    width: 160px;
    flex: none;
    color: var(--muted);
  }

  .who.unread {
    color: var(--fg);
    font-weight: bold;
  }

  .snippet {
    flex: 1;
    min-width: 0;
  }

  .date {
    flex: none;
    font-size: var(--fs-small);
  }

  .right {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
  }

  /* same column as in the list, so text lines up with or without the mark */
  .to-me {
    display: inline-block;
    width: 1.5ch;
    color: var(--muted);
  }

  .top {
    width: 100%;
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 16px;
    padding-top: 18px;
  }

  .to {
    margin-top: 4px;
    font-size: var(--fs-small);
  }

  .body {
    margin: 18px 0 8px;
    max-width: 68ch;
    font-size: var(--fs-body);
    line-height: 1.65;
  }

  .text,
  .quote {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .text + .text,
  .quote-toggle,
  .quote {
    margin-top: 1em;
  }

  .quote-toggle {
    display: block;
    font-size: var(--fs-small);
  }

  .quote-toggle:hover {
    color: var(--fg);
  }

  .quote {
    margin-top: 0.5em;
  }

  iframe {
    display: block;
    width: 100%;
    margin: 18px 0 8px;
    border: var(--line);
    background: #fff;
  }

  .files {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 8px 0;
  }

  .file:hover {
    color: var(--accent);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    padding: 14px 0 18px;
    font-size: var(--fs-small);
  }

  .actions button:hover b {
    color: var(--accent);
  }
</style>
