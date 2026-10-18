<script lang="ts">
  import Message from './Message.svelte';
  import { app } from './lib/state.svelte';
</script>

<section class="thread">
  {#if app.thread}
    <div class="head">
      <h1 class="selectable">{app.thread.subject || '(nessun oggetto)'}</h1>
      <p class="muted">
        {app.thread.messages.length}
        {app.thread.messages.length === 1 ? 'messaggio' : 'messaggi'} · {app.thread.participants.join(', ')}
      </p>
    </div>
    {#each app.thread.messages as message, index (message.id)}
      <Message {message} {index} />
    {/each}
  {:else}
    <p class="empty muted">{app.total ? '⏎ per aprire il thread selezionato' : ''}</p>
  {/if}
</section>

<style>
  .thread {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding-bottom: 48px;
  }

  .head {
    padding: 24px 32px 16px;
    border-bottom: var(--line);
  }

  h1 {
    font-size: 18px;
    font-weight: bold;
    line-height: 1.35;
  }

  .head p {
    margin-top: 6px;
  }

  .empty {
    margin-top: 40vh;
    text-align: center;
  }
</style>
