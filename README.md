<p align="center">
  <img src="assets/icon.svg" width="96" alt="">
  <br><br>
  <img src="assets/wordmark.svg" height="40" alt="lettera">
</p>

**lettera** is a minimal desktop email client: monospace type, thin 1px rules, no icons, no menus. It is driven from the keyboard; anything without a shortcut lives in the command palette (`:`). Every key hint on screen can also be clicked.

It supports two kinds of accounts, side by side:

- **Generic IMAP**: IMAP + SMTP with user and password (implicit TLS or STARTTLS);
- **Google**: Gmail over IMAP and SMTP with OAuth2 (`XOAUTH2`), no Gmail API.

It reads, searches (full text), archives, deletes, moves, replies, forwards and writes plain-text email. It syncs in the background, gets new mail through IDLE and shows a system notification. On Omarchy it takes its colors from the current theme and follows theme switches live.

```
inbox  12 unread · 240                                              simone@example.com
──────────────────────────────────────┬──────────────────────────────────────────────
 TODAY                                │  Q4 planning
▌■ Anna Rossi 3    Q4 planning        │  3 messages · Anna Rossi, Marco Bianchi
   Marco Bianchi   October invoice    │ ─────────────────────────────────────────────
                                      │  Giulia Verdi               sun 18 oct 10:02
──────────────────────────────────────┴──────────────────────────────────────────────
hjkl navigate · ⏎ open · e archive · r reply · c compose · / search · : commands   sync 22:14 ●
```

## Requirements

Building needs **Rust** (recent stable, via rustup), **Node.js** (≥ 22), [`just`](https://github.com/casey/just) and the Tauri CLI:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # if Rust is missing
cargo install just
cargo install tauri-cli --version "^2" --locked
```

System libraries (WebKitGTK 4.1 for the window, Secret Service for the keyring):

| system | packages |
|---|---|
| Arch / Omarchy | `sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl librsvg gnome-keyring` |
| Debian / Ubuntu | `sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev gnome-keyring` |
| macOS | Xcode Command Line Tools (`xcode-select --install`); the keyring is the Keychain |

On Linux a running Secret Service (gnome-keyring or KWallet) is required: passwords and tokens are stored only there.

## Installation

```sh
git clone <this repository> lettera && cd lettera
just build
```

`just build` produces `target/release/lettera` and the packages in `target/release/bundle/` (`.deb` and `.rpm` on Linux, `.app` and `.dmg` on macOS). On Arch you can simply copy the binary:

```sh
install -Dm755 target/release/lettera ~/.local/bin/lettera
```

### Flatpak

```sh
just flatpak                                  # builds lettera.flatpak
flatpak install --user lettera.flatpak
flatpak run app.lettera.desktop
```

This needs `flatpak` with the Flathub remote; the recipe uses `org.flatpak.Builder` and fetches GNOME 51, Rust and Node for the build by itself. Inside the sandbox, config and database live in `~/.var/app/app.lettera.desktop/` (separate from a native install); the app can read the Omarchy theme, use the keyring and notifications, and save or attach files from Downloads, Documents, Pictures and Desktop.

For development: `just dev` (window with live reload), `just test`, `just lint`, `just fmt`.

## First run

With no accounts, lettera shows the add-account form: pick `imap` or `google`, fill in the fields and press enter. For IMAP, both the IMAP and the SMTP login are checked before anything is saved; for Google the browser opens for consent. The same form comes back with `:` → `add account`.

## Configuration

The file is `~/.config/lettera/config.toml` (on macOS `~/Library/Application Support/lettera/config.toml`). It is reloaded as soon as you save it: adding or removing an `[[accounts]]` block starts or stops that account right away. `:` → `open config` opens it in your editor (terminal editors such as nvim open in a terminal).

The file is created with every option present as a commented placeholder: remove the `#` to turn one on. A typical file:

```toml
theme = "auto"            # auto | omarchy | dark | light
font = "JetBrains Mono"   # any installed monospace font
font_size = 13            # interface size in px; message bodies are 1px larger
sync_months = 48          # months of mail kept in sync, by arrival date (0 = all of it)
signature = """
Simone Silvetti
+39 333 1234567
"""

[[accounts]]
name = "Work"                      # shown in the app and used as sender name
type = "imap"
email = "name@example.com"
imap_host = "imap.example.com"
imap_port = 993                    # 993 = implicit TLS, other ports = STARTTLS
smtp_host = "smtp.example.com"
smtp_port = 465                    # 465 = implicit TLS, other ports = STARTTLS
# username = "name@example.com"    # if the login differs from the email
# imap_tls = "starttls"            # implicit | starttls, for non-standard ports
# smtp_tls = "starttls"
# sync_months = 12                 # overrides the global value (0 = all mail)
# signature = "Name · Work"        # overrides the global signature ("" = none)
# the password lives in the system keyring, never in this file

[[accounts]]
name = "Gmail"
type = "google"
email = "name@gmail.com"
client_id = "1234567890-abc.apps.googleusercontent.com"
client_secret = "GOCSPX-…"
# Gmail hosts and ports are built in; the refresh token lives in the keyring
```

Top-level options (`theme`, `font`, `font_size`, `sync_months`, `signature`) must come before the first `[[accounts]]` line: in TOML, everything after a table header belongs to that table.

The **signature** is added to every new message, reply and forward, after a `-- ` line (the standard separator many clients recognise); the cursor starts above it, and it can be edited or removed in each message. Multi-line strings go between `"""`.

**`sync_months`** limits sync to messages that arrived in the last N months, in every folder: older ones are never downloaded (not even their headers) and stay on the server only. As months go by the window moves forward and messages that fall out of it disappear from the local database, never from the server. Changing the value resyncs the account right away; widening the window brings older mail in.

Secrets are stored in the keyring under `lettera/<email>` (service `lettera`, user `<email>`). The local database is `~/.local/share/lettera/lettera.db`.

Useful environment variables:

- `LETTERA_HOME=/some/dir` — config and database in a single directory (to try things without touching your setup);
- `LETTERA_CA_FILE=/path/ca.pem` — extra CA certificates (servers with a private or self-signed CA);
- `RUST_LOG=lettera_core=debug` — more detailed logs (never passwords or tokens).

## Gmail without OAuth: app passwords

Gmail refuses the normal account password over IMAP (`Application-specific password required`). The quickest alternative to an OAuth client:

1. turn on 2-Step Verification on the Google account;
2. create an app password at <https://myaccount.google.com/apppasswords>;
3. add the account as `imap` with `imap.gmail.com` port 993 and `smtp.gmail.com` port 465 (or 587 with STARTTLS), using that 16-letter password (spaces do not matter).

With `imap_host = "imap.gmail.com"` lettera still applies Gmail semantics (archiving removes the label, no duplicate copy in *Sent Mail*), just as with `type = "google"`.

## Gmail: creating the OAuth client

lettera signs in to Gmail with OAuth2 using a client you create in your own Google Cloud project. It is free and done once.

1. Open <https://console.cloud.google.com/> and create a project (top menu → *New project*, e.g. “lettera”).
2. In *APIs & Services → Library* search for **Gmail API** and press *Enable*.
3. Go to *Google Auth Platform* (or *APIs & Services → OAuth consent screen*) and press *Get started*:
   - app name: `lettera`; support email: yours;
   - audience: **External**;
   - contact: your email; accept and create.
4. Under *Audience* (or *Test users*) add your Gmail address as a **test user**. Leave the app in **Testing** mode: personal use needs no verification.
5. Under *Data access* (or *Scopes*) press *Add or remove scopes* and add `https://mail.google.com/` by hand.
6. Under *Clients* press *Create client*, application type **Desktop app**, name `lettera`. Copy the **Client ID** and **client secret**.
7. In lettera: `add account` → `google` → name, email, client id, secret → `⏎ sign in with google`. The browser opens: pick the account, accept the “unverified app” warning (it is yours) and the consent screen. Back in lettera, sync starts on its own.

Notes:

- with the app in *Testing* mode Google expires the refresh token after **7 days**: when that happens the status line shows a sign-in error; run `add account` again with the same email to re-authorise (the account is not duplicated). To avoid it, press *Publish app* under *Audience*: it stays unverified (you will see the warning when signing in) but the token no longer expires;
- for Google Workspace accounts the administrator must allow IMAP access and third-party OAuth apps;
- archiving on Gmail removes the message from INBOX and leaves it in *All Mail*; deleting moves it to *Trash*. This works with Gmail's default IMAP settings (“when a message is deleted from the last visible IMAP folder: archive the message”).

## The message list

Each row shows sender (with the thread's message count), subject and date. Small muted marks:

- `■` on the left — unread;
- `›` before the subject — your account's address is in `To:` (sent to you directly, not as Cc or through a list);
- `*` before the subject — flagged;
- a paperclip before the date — the thread has attachments (guessed from the headers until the body is downloaded).

## Shortcuts

| key | action |
|---|---|
| `j` / `k`, `↓` / `↑` | move through the list, or through the messages when the thread has focus |
| `l` / `→` | from the list: open the selected thread and move focus to it · in the thread: expand the message |
| `h` / `←` | back to the list (the thread stays open) |
| `⏎` | open the thread · expand/collapse the message |
| `esc` | close / back / clear the search |
| `e` | archive |
| `#` | delete |
| `u` | mark as unread |
| `s` | toggle flag |
| `r` / `a` / `f` | reply / reply all / forward |
| `c` | compose |
| `/` | search (the list filters as you type, full-text search) |
| `:` | command palette |
| `v` | original HTML ↔ reading mode |
| `o` | save the message's attachment |
| `g i` `g a` `g s` `g t` `g d` | go to inbox, archive, sent, trash, drafts |
| `g g` / `G` | top / end of list |
| `n` / `p` | next / previous thread (inside an open thread) |
| `⌃1` … `⌃9` | switch account |
| `R` | sync now |

The pane with focus has the accent-colored bar (selected row or current message); the other one keeps a muted bar. `⌃` is the Ctrl key (Cmd on macOS). While composing: `⌃⏎` send · `⌃a` attach (click an attachment's name to remove it) · `⌃b` show the bcc field · `⌃q` show the quote · `esc` close (with unsaved changes it asks for a second `esc` and discards the draft). Drafts save themselves every 5 seconds and come back when you press `c` again (or `r`/`a`/`f` on the same message).

No shortcut fires while you type in a field, except `esc` and `⌃` combinations. Every command, including those without a key (`move to…`, `go to folder…`, `switch account…`, `open config`, `enable images`, `info`…), is in the palette.

Every key hint (the bottom bar, the actions under a message, the compose screen) is also a button: clicking it does the same as the shortcut. Hints that are not available right now are dimmed.

## The Omarchy theme

With `theme = "auto"` (or `"omarchy"`) lettera reads `~/.local/state/omarchy/current/theme/colors.toml` (or `~/.config/omarchy/current/theme/colors.toml`) and derives eight colors: `background`, `foreground`, `accent`, `red`, `green` from the file; `muted` from a `muted`/`comment`/`dim` key if present, otherwise a 55% text/background mix; borders and surfaces as 12% and 6% mixes. It watches the theme directory: when you switch theme in Omarchy the app recolors without a restart. A missing or broken file blocks nothing: lettera uses its built-in dark or light palette following the system preference. `theme = "dark"` or `"light"` force the built-in palettes.

## How it works

- `crates/core` is the engine, with no Tauri dependency: config, keyring, IMAP (`async-imap`), SMTP (`lettre`), OAuth, parsing (`mail-parser`), SQLite with FTS5, sync, threading, theme. It is tested on its own with `cargo test -p lettera-core`.
- `src-tauri` is a thin adapter: Tauri commands and events.
- `src/` is the Svelte 5 interface, with no domain logic: it asks for data, draws, sends commands.

Each account uses three IMAP connections: one for background sync (every 5 minutes and on demand), one for actions and on-demand bodies, one in IDLE on INBOX with automatic reconnection. Actions are optimistic: the list changes at once and the server right after; if something goes wrong the folder is resynced.

Tests: `just test` runs unit tests, parsing of 14 `.eml` fixtures, threading, sync against a fake IMAP server (including `UIDVALIDITY` changes) and the frontend tests. `just test-integration` starts Dovecot and Mailpit in Docker (`docker-compose.test.yml`) and runs the whole engine against real servers: login, sync, IDLE, reply with `In-Reply-To`/`References` and a PDF attachment, copy to Sent, archive, trash, STARTTLS fallback.

## Known limits

- IMAP/SMTP and Gmail only; no JMAP, Exchange/EWS, POP3, PGP, calendar, contacts, filters.
- Plain-text writing only; forwarding quotes the original text but does not carry its attachments.
- Drafts stay local (they are not saved to the server's Drafts folder).
- Sync covers every folder, including *All Mail* on Gmail, within the `sync_months` window (48 months by default); older mail cannot be reached from the app. No CONDSTORE/QRESYNC.
- Search works on the current folder and finds body text only for messages whose body has been downloaded (automatic prefetch of the last 30 days of INBOX).
- Non-ASCII folder names in modified UTF-7 are shown as they arrive.
- Inline `cid:` images of the original HTML are not shown.
- Shortcuts are not configurable; a single window.
- Tested on Linux (Omarchy/Hyprland); on macOS it builds without platform-specific code but has not been tried.

## License

MIT, see [LICENSE](LICENSE). The Special Elite font in `assets/fonts/` ships with its own Apache 2.0 license.
