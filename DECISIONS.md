# Decisions

Non-obvious choices, one line each. Where a choice was ambiguous, the simplest option was taken.

## Environment and dependencies

- Rust installed with rustup in `~/.cargo` without touching PATH; `just` and `tauri-cli` via `cargo install`.
- TypeScript 6 rather than 7: svelte-check does not support TS 7 yet without installing two versions.
- `keyring` 4 with the `v1` feature: Secret Service (zbus) on Linux, Keychain on macOS; service `lettera`, user = email.
- Keyring operations (blocking) run in `spawn_blocking`.
- The frontend depends only on `@tauri-apps/api`: file dialogs, opening links and notifications are done in Rust.
- `oauth2` uses its re-exported `reqwest`, to avoid two reqwest versions.
- `mail-parser` with `full_encoding` for less common non-UTF-8 charsets.
- Frontend tests with Node's built-in runner (native type stripping): no test dependencies.

## Network and security

- TLS mode inferred from the port (993/465 implicit, others STARTTLS); `imap_tls`/`smtp_tls` in the config for non-standard ports.
- IMAP connect, TLS, greeting and login share a 45 s timeout: a wrong TLS mode becomes a readable error, not a hang.
- SMTP: if implicit TLS gets clear text back (a STARTTLS port) it retries with STARTTLS, TLS still mandatory; adding an account also checks the SMTP login and saves the mode that worked.
- Mozilla TLS roots (`webpki-roots`), plus the PEM CAs named by `LETTERA_CA_FILE` (servers with a private CA, tests).
- `LETTERA_HOME` puts config and database in the same directory (trials and tests).
- An IMAP account is saved only after a real login: a wrong password reaches neither the config nor the keyring.
- Links open in the system browser only if `http`, `https` or `mailto`.
- Original HTML: ammonia when stored (inline styles and presentational attributes allowed), remote images stripped and CSP `default-src 'none'` when shown.
- iframe `sandbox="allow-same-origin"` without `allow-scripts`: no script runs, but the frontend intercepts link clicks and hands them to the browser.
- `:enable images` applies to that message only and for the current session only.
- Tauri adds no nonce to `style-src` (`dangerousDisableAssetCspModification: ["style-src"]`): with a nonce the browser would ignore `'unsafe-inline'` and block inline styles (virtual list, email HTML). Scripts stay nonce-protected.
- Gmail's "Application-specific password required" becomes a message with instructions for app passwords.

## Sync

- Three connections per account: background sync, foreground actions and bodies, IDLE on INBOX.
- A single `UID FETCH <set> (FLAGS)` per folder gives new UIDs, updated flags and vanished messages at once; new ones are fetched newest first, 200 at a time.
- Every UID missing locally is new (not only those ≥ UIDNEXT): an interrupted first sync completes by itself.
- Every selectable folder is synced, *All Mail* on Gmail included; envelopes only.
- Sync window `sync_months` (default 48, 0 = everything, per-account override): `UID SEARCH SINCE` by arrival date, then flags from `<lowest UID>:*`; messages leaving the window are deleted locally only.
- Bodies on demand plus prefetch of the last 30 days of INBOX, at most 150 per round.
- `has_attachments` is guessed from the header (`multipart/mixed`) until the body arrives, then corrected by parsing; no BODYSTRUCTURE.
- Attachments are not stored on disk: the message is downloaded again when the user saves an attachment.
- Optimistic actions: SQLite changes at once, the server afterwards; UIDs being moved are skipped by sync so they do not reappear.
- If an action fails on the server the account is resynced: the server is the truth and the list becomes consistent again.
- After each action the folders involved (source and destination) are resynced.
- Queued sync requests are merged into one.
- Sync errors: retry with growing delay (30 s … 5 min); IDLE reconnects from 2 s to 5 min.
- Servers without IDLE: NOOP and INBOX sync every 60 s.
- A role (sent, trash, …) goes to one folder only; SPECIAL-USE wins over names.
- Generic IMAP archive: the folder with the `archive` role, otherwise `Archive` created on first use.
- Delete: moves to trash (`Trash` created if missing); deleting from trash is permanent.
- Gmail: archive = `\Deleted` + `UID EXPUNGE` in the folder; from Spam or Trash it moves to *All Mail* instead, since an expunge there would delete for good.
- Gmail semantics also for `imap` accounts on `imap.gmail.com` (app passwords).
- Gmail: no APPEND after sending (Gmail does it) but *Sent Mail* is resynced.

## Data and threading

- `thread_id` is the string `<account_id>:<root message-id>`; unique across accounts, so `get_thread` only needs the id.
- Replies that arrive before their parent are "adopted" into the parent's thread when it arrives.
- Messages without a Message-ID: stable synthetic id (FNV-1a of date, sender and subject).
- A thread shows messages from every folder of the account (e.g. replies in Sent), once per Message-ID.
- Addresses stored as JSON `{name, addr}` so names with commas survive.
- FTS5 search in the current folder: every word as a prefix, ANDed.
- Date groups computed in Rust in local time: today, yesterday, last 7 days, earlier.
- Numbered SQL migrations applied through `PRAGMA user_version`; database in WAL mode.
- The config is edited as text (adding or removing an `[[accounts]]` block) so the user's comments survive.
- New config files and new account blocks list every unset option as a commented placeholder.
- An account removed from the config is stopped and deleted from the database on reload.

## Composing

- Local-only drafts in SQLite; one per (account, kind, source message): `c`, `r`, `a`, `f` resume the existing one.
- The quote is kept apart from the body, hidden behind `⌃q` and always appended on send.
- Signature from the config (`signature`, global or per account, "" = none): put in the draft body after `-- `, above the quote, so it can be seen and edited before sending.
- Forwarding quotes the original text but does not carry its attachments.
- `esc`: if everything is saved it closes and the draft stays; with unsaved changes it asks for a second `esc` that discards the draft.
- `send` waits for SMTP delivery (a deliberate exception to the 50 ms rule) to show any error right away.
- Bcc recipients only go into the SMTP envelope, never into headers.
- Message-ID generated with the sender's domain.

## Interface

- The whole app — interface, messages, errors, logs, config file, documentation — is in English.
- Commands and shortcuts come from a single registry; shortcuts have a *scope* (list, thread, mail, compose, palette, search, global).
- Tauri commands take snake_case arguments (`rename_all`), as in the spec.
- Commands beyond the spec's list: `prepare_draft`, `discard_draft`, `pick_files`, `open_url`, `enable_images`, `suggest_contacts`, `app_info`; extra events: `body:ready`, `accounts:changed`.
- Key hints are also buttons that run the registry command; disabled when the command is not available.
- Removing an attachment while composing: click its name.
- `open config` uses `gio open` on Linux (terminal editors open in a terminal), the opener plugin as a fallback and inside Flatpak.
- Vim-style panes: `h`/`←` focus the list without closing the thread, `l`/`→` open/focus the selected thread, and in the thread expand the current message (never collapse: that stays on `⏎`). The inactive pane's selection bar turns muted.
- Opening a thread marks it read; `u` marks it unread and closes the thread.
- Archiving or deleting with a thread open opens the next thread.
- Virtual list with fixed-height rows (38 px), group headers included.
- The palette shows only the commands available in context; navigation keys are not listed.
- `:info` shows the wordmark, version, license and config path.
- Folders with a role get English labels (inbox, sent, drafts, trash, archive, spam, all mail).
- Dates in the list: the year appears only if it is not the current one.
- Square unread dot: no `border-radius`, not even there.
- List marks: muted `›` when the account's address is in `To:` of any message in the thread (Gmail's convention), in a reserved column so subjects line up; a 1.5px-stroke paperclip before the date for attachments — the only drawn glyph in the app, asked for by the user.
- The Google form asks for client id and secret: before the account exists there is nowhere else to take them from.
- OAuth with `access_type=offline`, `prompt=consent` and `login_hint`, to always get a refresh token for the right email.
- The add form suggests `imap.` and `smtp.` of the email's domain; the TLS mode follows the port but can be switched with a click.
- Notifications are sent by the Rust process, only for unread messages that arrived through IDLE.

## Packaging and identity

- Flatpak on GNOME 51 with a build that downloads dependencies (`--share=network`): a local package, not ready for Flathub; data in the sandbox, not shared with a native install.
- `deb` and `rpm` bundles on Linux, `app` and `dmg` on macOS; no AppImage (it downloads tools during the build).
- Icon hand-drawn in SVG and converted with `cargo tauri icon`; set reduced to desktop icons.
- Wordmark: Special Elite (Apache 2.0) converted to paths with fontTools, color `currentColor`.
- Integration tests: Dovecot and Mailpit in Docker with a test CA in the repository (the CA's private key was discarded).
