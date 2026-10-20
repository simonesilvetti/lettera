<script lang="ts">
  import { api } from './lib/api';
  import { app, flash, loadAccounts, selectAccount } from './lib/state.svelte';
  import type { NewAccount } from './lib/types';
  import wordmark from '../assets/wordmark.svg?raw';
  import Logo from './Logo.svelte';

  let kind = $state<'imap' | 'google'>('google');
  let name = $state('');
  let email = $state('');
  let imapHost = $state('');
  let imapPort = $state(993);
  let smtpHost = $state('');
  let smtpPort = $state(465);
  let username = $state('');
  let password = $state('');
  let imapTls = $state<'implicit' | 'starttls'>('implicit');
  let smtpTls = $state<'implicit' | 'starttls'>('implicit');
  let busy = $state(false);
  let error = $state('');
  let first = $state<HTMLInputElement>();

  $effect(() => {
    first?.focus();
  });

  // guess servers from the domain the first time the email is typed
  function guess() {
    const domain = email.split('@')[1];
    if (!domain) return;
    if (!imapHost) imapHost = `imap.${domain}`;
    if (!smtpHost) smtpHost = `smtp.${domain}`;
  }

  // default TLS mode follows the port, as in the backend
  const tlsFor = (port: number, implicitPort: number) => (Number(port) === implicitPort ? 'implicit' : 'starttls');
  $effect(() => {
    imapTls = tlsFor(imapPort, 993);
  });
  $effect(() => {
    smtpTls = tlsFor(smtpPort, 465);
  });

  // a google account is set up as imap on gmail's servers with an app
  // password: the user only sees email and password (oauth stays available
  // through the config file, see the readme)
  const APP_PASSWORDS = 'https://myaccount.google.com/apppasswords';

  function account(): NewAccount {
    if (kind === 'google')
      return {
        name,
        kind: 'imap',
        email,
        imap_host: 'imap.gmail.com',
        imap_port: 993,
        smtp_host: 'smtp.gmail.com',
        smtp_port: 465,
        // google shows the app password in groups of four
        password: password.replace(/\s+/g, ''),
      };
    return {
      name,
      kind,
      email,
      imap_host: imapHost,
      imap_port: Number(imapPort),
      smtp_host: smtpHost,
      smtp_port: Number(smtpPort),
      username: username || undefined,
      password,
      imap_tls: imapTls === tlsFor(imapPort, 993) ? undefined : imapTls,
      smtp_tls: smtpTls === tlsFor(smtpPort, 465) ? undefined : smtpTls,
    };
  }

  async function done(addr: string) {
    app.setupOpen = false;
    await loadAccounts();
    await selectAccount(addr);
    app.screen = 'inbox';
    flash(`account ${addr} added, syncing…`);
  }

  async function submit(e: Event) {
    e.preventDefault();
    if (busy) return;
    error = '';
    busy = true;
    const addr = email.trim();
    try {
      await api.addAccount(account());
      await done(addr);
    } catch (err) {
      error = String(err);
    }
    busy = false;
  }

  function cancel(e: KeyboardEvent) {
    if (e.key === 'Escape' && app.accounts.length) {
      app.setupOpen = false;
      app.screen = 'inbox';
    }
  }
</script>

<svelte:window onkeydown={cancel} />

<div class="setup">
  <form onsubmit={submit}>
    <span class="wordmark"><Logo size={30} />{@html wordmark}</span>
    <p class="muted intro">add an account</p>

    <div class="rows">
    <div class="row">
      <span class="muted">type</span>
      <span class="kinds">
        <button type="button" class:on={kind === 'google'} onclick={() => (kind = 'google')}>google</button>
        <span class="muted">·</span>
        <button type="button" class:on={kind === 'imap'} onclick={() => (kind = 'imap')}>imap</button>
      </span>
    </div>
    <label class="row"><span class="muted">name</span><input bind:this={first} bind:value={name} placeholder="your name, as recipients see it" required /></label>
    <label class="row">
      <span class="muted">email</span>
      <input
        bind:value={email}
        onblur={() => kind === 'imap' && guess()}
        placeholder={kind === 'google' ? 'name@gmail.com' : 'name@example.com'}
        required
      />
    </label>

    {#if kind === 'imap'}
      <label class="row">
        <span class="muted">imap</span>
        <input bind:value={imapHost} placeholder="imap.example.com" required />
        <input class="port" bind:value={imapPort} inputmode="numeric" />
        <button type="button" class="tls" onclick={() => (imapTls = imapTls === 'implicit' ? 'starttls' : 'implicit')}>
          {imapTls === 'implicit' ? 'tls' : 'starttls'}
        </button>
      </label>
      <label class="row">
        <span class="muted">smtp</span>
        <input bind:value={smtpHost} placeholder="smtp.example.com" required />
        <input class="port" bind:value={smtpPort} inputmode="numeric" />
        <button type="button" class="tls" onclick={() => (smtpTls = smtpTls === 'implicit' ? 'starttls' : 'implicit')}>
          {smtpTls === 'implicit' ? 'tls' : 'starttls'}
        </button>
      </label>
      <label class="row">
        <span class="muted">user</span>
        <input bind:value={username} placeholder={email || 'same as email'} />
      </label>
      <label class="row">
        <span class="muted">password</span>
        <input type="password" bind:value={password} required />
      </label>
    {:else}
      <label class="row">
        <span class="muted">password</span>
        <input type="password" bind:value={password} placeholder="app password, see below" required />
      </label>
    {/if}
    </div>
    {#if kind === 'imap'}
      <p class="note muted">click tls/starttls to switch mode · the password goes into the system keyring</p>
    {:else}
      <ol class="note steps muted">
        <li>
          google does not accept your usual password here: create an app password at
          <a href={APP_PASSWORDS} onclick={(e) => (e.preventDefault(), api.openUrl(APP_PASSWORDS))}
            >myaccount.google.com/apppasswords</a
          >
        </li>
        <li>type any name (e.g. “lettera”), press create and paste the 16 letters above</li>
        <li>if the page says app passwords are not available, turn on 2-step verification first</li>
      </ol>
    {/if}

    <div class="actions">
      <button type="submit" class="hint" disabled={busy}>
        <b>⏎</b>
        {#if busy}
          checking sign-in…
        {:else}
          add
        {/if}
      </button>
    </div>
    {#if error}<p class="error">{error}</p>{/if}
  </form>
</div>

<style>
  .setup {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow-y: auto;
    height: calc(100% - 32px);
  }

  form {
    width: 520px;
    max-width: calc(100% - 32px);
  }

  .wordmark {
    display: flex;
    align-items: center;
    gap: 12px;
    color: var(--fg);
  }

  .wordmark > :global(svg:last-child) {
    height: 28px;
    width: auto;
    display: block;
  }

  .intro {
    margin: 16px 0 20px;
  }

  .row {
    height: 40px;
    display: flex;
    align-items: center;
    gap: 12px;
    border-bottom: var(--line);
  }

  .rows {
    border-top: var(--line);
  }

  .row > span:first-child {
    width: 72px;
    flex: none;
  }

  .row input {
    flex: 1;
    min-width: 0;
    height: 100%;
  }

  .row .port {
    flex: none;
    width: 56px;
    text-align: right;
  }

  .tls {
    width: 64px;
    flex: none;
    text-align: right;
    color: var(--muted);
  }

  .tls:hover {
    color: var(--fg);
  }

  .kinds {
    display: flex;
    gap: 8px;
  }

  .kinds button {
    color: var(--muted);
  }

  .kinds button.on {
    color: var(--fg);
    text-decoration: underline;
    text-decoration-color: var(--accent);
    text-underline-offset: 4px;
  }

  .note {
    margin-top: 10px;
    font-size: var(--fs-small);
  }

  .steps {
    padding-left: 18px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .actions {
    margin-top: 22px;
  }

  .actions button:hover b {
    color: var(--accent);
  }

  .error {
    margin-top: 12px;
    color: var(--red);
    white-space: pre-wrap;
    user-select: text;
  }
</style>
