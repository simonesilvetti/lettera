<script lang="ts">
  // The privacy report of a message (`t`): what in it can track the reader.
  import type { HostCount, PrivacyReport } from './lib/types';

  let { report, protection }: { report: PrivacyReport | null; protection: boolean } = $props();

  const sum = (list: HostCount[]) => list.reduce((n, h) => n + h.count, 0);
  const hosts = (list: HostCount[]) => list.map((h) => h.host).join(', ');
  const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

  const pixels = $derived(report ? sum(report.pixels) : 0);
  const personal = $derived(report ? sum(report.personal_images) : 0);
  const generic = $derived(report ? sum(report.generic_images) : 0);

  /** One sentence for the whole message. */
  const verdict = $derived.by(() => {
    if (!report) return '';
    const l = report.links;
    const parts: string[] = [];
    parts.push(pixels ? `${plural(pixels, 'tracking pixel')}, removed` : 'no tracking pixel');
    if (personal) parts.push(`${plural(personal, 'image')} with a code for you`);
    else if (generic) parts.push('the images are the same for everyone');
    if (l.personal || l.counters) {
      parts.push(
        protection && !l.counters
          ? 'the links would identify you, they open without tracking'
          : 'the links identify you if you click them',
      );
    } else if (l.total) parts.push('the links carry no tracking');
    const s = parts.join('; ');
    return s.charAt(0).toUpperCase() + s.slice(1) + '.';
  });
</script>

<section class="report">
  <p class="head muted">privacy report</p>
  {#if !report}
    <p class="muted">reading…</p>
  {:else}
    <p class="verdict">{verdict}</p>
    <dl>
      <dt class="muted">images</dt>
      <dd>
        {#if generic + personal + pixels === 0}
          <span class="muted">no remote images</span>
        {/if}
        {#if generic}
          <span>{generic} the same for everyone <span class="muted">{hosts(report.generic_images)}</span></span>
        {/if}
        {#if personal}
          <span>
            {personal} with a code for you <span class="muted">{hosts(report.personal_images)}</span>
            <span class="muted">· {protection ? 'blocked, M loads them' : 'loaded by m'}</span>
          </span>
        {/if}
        {#if pixels}
          <span>
            {plural(pixels, 'tracking pixel')} <span class="muted">{hosts(report.pixels)} · always removed</span>
          </span>
        {/if}
      </dd>
      <dt class="muted">links</dt>
      <dd>
        {#if report.links.total === 0}
          <span class="muted">no links</span>
        {:else}
          <span>
            {report.links.personal
              ? `${report.links.personal} of ${report.links.total} carry a personal code`
              : `${plural(report.links.total, 'link')}, none with a personal code`}
            {#if report.links.params.length}
              <span class="muted">
                {report.links.params.join(', ')} · {protection ? 'removed when you click' : 'kept, protection is off'}
              </span>
            {/if}
          </span>
          {#if report.links.counters}
            <span>
              {report.links.counters} through a click counter
              <span class="muted">{hosts(report.links.counter_hosts)} · cannot be cleaned</span>
            </span>
          {/if}
          {#if report.links.redirects}
            <span>
              {report.links.redirects} through a redirect
              <span class="muted">· {protection ? 'opened straight to the destination' : 'kept, protection is off'}</span>
            </span>
          {/if}
        {/if}
      </dd>
      <dt class="muted">sent via</dt>
      <dd>
        {#if report.sent_via}
          <span>
            {report.sent_via.via}
            {#if report.sent_via.bulk}<span class="muted">· mass mailing</span>{/if}
          </span>
        {:else}
          <span class="muted">headers not available</span>
        {/if}
      </dd>
    </dl>
  {/if}
</section>

<style>
  .report {
    margin-top: 18px;
    padding: 10px 0 12px;
    border-top: var(--line);
    border-bottom: var(--line);
    font-size: var(--fs-small);
    max-width: 80ch;
  }

  .head {
    margin-bottom: 6px;
  }

  .verdict {
    margin-bottom: 10px;
  }

  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    column-gap: 16px;
    row-gap: 6px;
  }

  dd {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    overflow-wrap: anywhere;
  }
</style>
