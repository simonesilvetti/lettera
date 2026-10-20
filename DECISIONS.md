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
- iframe `sandbox="allow-same-origin allow-scripts"`: WebKit runs a parent-added click listener inside the frame only with `allow-scripts` (without it, links in HTML mode did nothing). The email still runs no code: ammonia strips scripts and handlers when stored and the frame's CSP is `default-src 'none'` (checked in WebKitGTK with an injected `<script>`, `onerror` and `javascript:` link). Every link click is intercepted and only `http`, `https`, `mailto` reach the browser.
- `:enable images` applies to that message only and for the current session only.
- Tauri adds no nonce to `style-src` (`dangerousDisableAssetCspModification: ["style-src"]`): with a nonce the browser would ignore `'unsafe-inline'` and block inline styles (virtual list, email HTML). Scripts stay nonce-protected.
- Gmail's "Application-specific password required" becomes a message with instructions for app passwords.
- `save_attachment` takes no path from the webview: the destination always comes from the system dialog, so the UI cannot write files by itself.
- The config directory and data directory are 0700, `config.toml` and the database 0600 (they hold an OAuth client secret and the mail); existing files are tightened at startup.
- Message-IDs lose whitespace, control characters and `<>` both when parsed and when written into `In-Reply-To`/`References`, so a crafted id cannot inject a header into a reply.
- Inline `style` attributes containing `javascript:`, `vbscript:`, `expression(`, `behavior:` or `-moz-binding` are dropped.
- Known, accepted: files listed in a draft's attachments are read from whatever paths the UI sends (they come from the file picker or a saved draft); a compromised webview could attach any readable file. The webview only runs the app's own bundled scripts (`script-src 'self'`).

## Sync

- Three connections per account: background sync, foreground actions and bodies, IDLE on INBOX.
- A single `UID FETCH <set> (FLAGS)` per folder gives new UIDs, updated flags and vanished messages at once; new ones are fetched newest first, 200 at a time.
- Every UID missing locally is new (not only those ≥ UIDNEXT): an interrupted first sync completes by itself.
- Every selectable folder is synced, *All Mail* on Gmail included; envelopes only.
- Sync window `sync_months` (default 48, 0 = everything, per-account override): `UID SEARCH SINCE` by arrival date, then flags from `<lowest UID>:*`; messages leaving the window are deleted locally only.
- Bodies on demand plus prefetch of the last 30 days of INBOX and Sent, at most 150 per folder per round, of the newest 20 threads of a folder when it is opened (on the sync connection), and of the selected thread (last 10 messages, 150 ms after the selection rests).
- The action connection opens when the account starts and is pinged every 4 idle minutes (replaced at once if dead), so a click never waits for a login.
- Body downloads go ahead of other queued actions, are never queued twice, and use the copy in the folder already selected on the connection (else INBOX), one `UID FETCH` per folder.
- On Gmail a flag change is sent once per message (labels share flags); other servers get it on every copy.
- `has_attachments` is guessed from the header (`multipart/mixed`) until the body arrives, then corrected by parsing; no BODYSTRUCTURE.
- Attachments are not stored on disk: the message is downloaded again when the user saves an attachment.
- Optimistic actions: SQLite changes at once, the server afterwards; UIDs being moved are skipped by sync so they do not reappear.
- Flag changes apply to every copy of the message (same Message-ID in other folders), locally and on the server: the thread view shows one copy, the list may show another.
- A panic inside a sync or action job is turned into an error, so the worker keeps running.
- If an action fails on the server the account is resynced: the server is the truth and the list becomes consistent again.
- After each action the folders involved (source and destination) are resynced.
- Queued sync requests are merged into one.
- Sync errors: retry with growing delay (30 s … 5 min); IDLE reconnects from 2 s to 5 min.
- Servers without IDLE: NOOP and INBOX sync every 60 s.
- Dead connections are detected, not waited out: TCP keepalive (60 s, then 3 probes 10 s apart) and a 30 s TCP user timeout on every IMAP socket, and a NOOP (5 s limit) before reusing a sync or action connection idle for over 5 minutes.
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

- Drafts live in SQLite (autosave every 5 s) and, on closing the composer, as a copy in the server's Drafts folder (`\Draft`, a fresh Message-ID each time, the previous copy removed by `UID SEARCH HEADER Message-ID`; on Gmail moved to the trash, since an expunge would leave it in All Mail). The local row keeps kind, source and quote and is found again through that Message-ID. `c` always starts a new message; `r`, `a`, `f` resume the reply/forward already started on that message; older drafts are reopened from the Drafts folder with `i`. Drafts made by other clients open too, rebuilt from the message (attachments copied under `data_dir/drafts/`).
- List rows show the newest message of the thread; in Sent and Drafts the sender column shows `to <first recipient>`.
- Contacts for autocompletion: senders plus recipients of sent mail (weighing more), read once and kept 10 minutes.
- Threads of messages without reply headers are merged when they match, so threading does not depend on arrival order.
- Html-only mail is read as text with layout tables as one column (`html2text` raw mode); links stay as footnotes.
- Tauri commands that touch the database are `async`, off the main thread.
- The quote is kept apart from the body, hidden behind `⌃q` and always appended on send.
- Signature from the config (`signature`, global or per account, "" = none): put in the draft body after `-- `, above the quote, so it can be seen and edited before sending.
- Forwarding quotes the original text but does not carry its attachments.
- A draft keeps the `In-Reply-To`/`References` of the message it answers from the start, so it can be saved and sent after that message left the database.
- Replying to one's own message goes to its recipients (oneself excluded), as other clients do.
- Sending stops when a recipient entry is not an address, instead of leaving it out.
- Storing or discarding a draft changes the local Drafts folder at once (same row id, body included, a local UID ≥ `0xFFF0_0000` until sync sees the server copy and the row takes its UID): the composer never waits for the server.
- `esc` in compose saves to the Drafts folder and closes (asked for by the user); a draft left blank or untouched is not stored. Discarding is `⌃d`, pressed twice.
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
- The sync status in the bottom bar is a button for `sync now`.
- Resizable list pane: a 7px invisible grab area on the border (accent line on hover), list ≥ 300px and thread ≥ 360px, double-click resets to 420px; width kept in `localStorage` (a per-machine convenience). The sender column shrinks with the list (`clamp(96px, 32%, 150px)`).
- `open config` uses `gio open` on Linux (terminal editors open in a terminal), the opener plugin as a fallback and inside Flatpak.
- Vim-style panes: `h`/`←` focus the list without closing the thread, `l`/`→` open/focus the selected thread, and in the thread expand the current message; in the thread `h`/`←` first collapses an open message, then goes back to the list. The inactive pane's selection bar turns muted.
- Opening a thread marks it read; `u` marks it unread and closes the thread.
- Archiving or deleting with a thread open opens the next thread.
- Virtual list with fixed-height rows (38 px), group headers included.
- The palette shows only the commands available in context; navigation keys are not listed.
- `:info` shows the wordmark, version, license and config path.
- The top bar has the icon and the wordmark centred on the window, drawn with theme tokens (envelope in `--accent`, typewriter and wordmark in `--fg`), hidden while searching; clicking it opens `:info`.
- Folders with a role get English labels (inbox, sent, drafts, trash, archive, spam, all mail).
- Dates in the list: the year appears only if it is not the current one.
- Square unread dot: no `border-radius`, not even there.
- List marks: muted `›` when the account's address is in `To:` of any message in the thread (Gmail's convention), in a reserved column so subjects line up; a 1.5px-stroke paperclip before the date for attachments — the only drawn glyph in the app, asked for by the user. Same marks on each message of an open thread (`to_me` computed in the frontend from `MessageView.to`).
- The form's `google` choice is an IMAP account on Gmail's servers with an app password: email and password are all a user can be asked for without lettera owning a Google OAuth client (a published client with the restricted Gmail scope needs Google's verification). OAuth with one's own client stays available through the config file and `sign in with google`.
- OAuth with `access_type=offline`, `prompt=consent` and `login_hint`, to always get a refresh token for the right email.
- The add form suggests `imap.` and `smtp.` of the email's domain; the TLS mode follows the port but can be switched with a click.
- Notifications are sent by the Rust process, only for unread messages that arrived through IDLE; one per sync ("N new messages" when several arrive together), with the app icon (written to the data dir so the daemon can read it). On Linux they go through `notify-rust` directly, because the Tauri plugin has no click events there: clicking raises the window and opens that message in its INBOX (`open_message` event).
- Closing the window hides it while the tray icon is there, so sync and notifications go on; *Quit* in the tray menu (or `quit` in the palette) exits. Without a tray host, closing quits. Left click on the icon shows the window, right click opens *Open lettera / Compose email / Open config file / Quit*.
- On Linux the tray icon is a StatusNotifierItem via `ksni`, not Tauri's tray: libappindicator never reports a left click. Tauri's tray is used on the other platforms.
- One instance (`tauri-plugin-single-instance`): a second launch shows the running window. Off in debug builds and with `LETTERA_HOME`, so development copies stay independent.
- `autostart = true` by default: lettera writes `~/.config/autostart/lettera.desktop` (started with `--hidden`) and removes it when set to false, live on config reload. Only the Flatpak and binaries under `/usr` or `/opt` manage it, never development builds; the Flatpak writes it itself (`xdg-config/autostart:create`) because Omarchy has no Background portal.
- Left column with four folder icons (inbox, drafts, sent, archive), Tabler outline icons that tilt 6° and turn accent on hover, like the links on simonesilvetti.com; asked for by the user, the second exception to "no icons" after the paperclip.
- `⌃s` in compose saves the draft and goes back to the list (same as `esc` since drafts go to the server).

## Packaging and identity

- Flatpak on GNOME 51 with a build that downloads dependencies (`--share=network`): a local package, not ready for Flathub; data in the sandbox, not shared with a native install.
- `deb` and `rpm` bundles on Linux, `app` and `dmg` on macOS; no AppImage (it downloads tools during the build).
- Icon hand-drawn in SVG and converted with `cargo tauri icon`; set reduced to desktop icons. Since 2026-10-19 the typewriter types an envelope (flap cut out as a V) instead of a sheet, so it reads as an email client; chosen by the user among three drafts. Its colors are the user's Omarchy theme at the time (background `#05182e`, typewriter `#f6dcac`, envelope `#faa968`), so the static icon matches the in-app logo (`src/Logo.svelte`, drawn with theme tokens and shown in the top bar and on the setup screen).
- Wordmark: Special Elite (Apache 2.0) converted to paths with fontTools, color `currentColor`.
- Integration tests: Dovecot and Mailpit in Docker with a test CA in the repository (the CA's private key was discarded).
