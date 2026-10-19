<p align="center">
  <img src="assets/icon.svg" width="96" alt="">
  <br><br>
  <img src="assets/wordmark.svg" height="40" alt="lettera">
</p>

**lettera** è un client email desktop minimale: tipografia monospace, righe sottili da 1px, niente icone, niente menu. Si usa da tastiera; tutto ciò che non ha una scorciatoia sta nella palette dei comandi (`:`).

Supporta due tipi di account, anche insieme:

- **IMAP generico**: IMAP + SMTP con utente e password (TLS implicito o STARTTLS);
- **Google**: Gmail via IMAP e SMTP con OAuth2 (`XOAUTH2`), senza Gmail API.

Legge, cerca (full-text), archivia, elimina, sposta, risponde, inoltra e scrive email in testo semplice. Sincronizza in background, riceve la posta nuova con IDLE e manda una notifica di sistema. Su Omarchy prende i colori dal tema corrente e li aggiorna appena cambi tema.

```
inbox  12 non letti · 240                                           simone@esempio.it
──────────────────────────────────────┬──────────────────────────────────────────────
 OGGI                                 │  Pianificazione Q4
▌■ Anna Rossi 3    Pianificazione Q4  │  3 messaggi · Anna Rossi, Marco Bianchi
   Marco Bianchi   Fattura ottobre    │ ─────────────────────────────────────────────
                                      │  Giulia Verdi               dom 18 ott 10:02
──────────────────────────────────────┴──────────────────────────────────────────────
j/k naviga · ⏎ apri · e archivia · r rispondi · c scrivi · / cerca · : comandi   sync 22:14 ●
```

## Requisiti

Per compilare servono **Rust** (stabile recente, via rustup), **Node.js** (≥ 22), [`just`](https://github.com/casey/just) e la CLI di Tauri:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # se manca Rust
cargo install just
cargo install tauri-cli --version "^2" --locked
```

Librerie di sistema (WebKitGTK 4.1 per la finestra, Secret Service per il keyring):

| sistema | pacchetti |
|---|---|
| Arch / Omarchy | `sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl librsvg gnome-keyring` |
| Debian / Ubuntu | `sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev gnome-keyring` |
| macOS | Xcode Command Line Tools (`xcode-select --install`); il keyring è il Portachiavi |

Su Linux serve un servizio Secret Service attivo (gnome-keyring o KWallet): password e token stanno solo lì.

## Installazione

```sh
git clone <questo repository> lettera && cd lettera
just build
```

`just build` produce `target/release/lettera` e i pacchetti in `target/release/bundle/` (`.deb` e `.rpm` su Linux, `.app` e `.dmg` su macOS). Su Arch puoi semplicemente copiare il binario:

```sh
install -Dm755 target/release/lettera ~/.local/bin/lettera
```

### Flatpak

```sh
just flatpak                                  # crea lettera.flatpak
flatpak install --user lettera.flatpak
flatpak run app.lettera.desktop
```

Serve `flatpak` con il remote Flathub; la ricetta usa `org.flatpak.Builder` e scarica da sola GNOME 51, Rust e Node per la build. Nel sandbox config e database stanno in `~/.var/app/app.lettera.desktop/` (separati da quelli dell'installazione nativa); l'app può leggere il tema di Omarchy, usare il keyring e le notifiche, e salvare o allegare file da Scaricati, Documenti, Immagini e Scrivania.

Per sviluppare: `just dev` (finestra con ricarica automatica), `just test`, `just lint`, `just fmt`.

## Primo avvio

Senza account, lettera mostra il form di aggiunta: scegli `imap` o `google`, inserisci i dati e premi invio. Per IMAP la password viene verificata con un login reale prima di salvare; per Google si apre il browser per il consenso. Lo stesso form si riapre con `:` → `aggiungi account`.

## Configurazione

Il file è `~/.config/lettera/config.toml` (su macOS `~/Library/Application Support/lettera/config.toml`). Viene ricaricato appena lo salvi: aggiungere o togliere un blocco `[[accounts]]` avvia o ferma subito quell'account. `:` → `apri config` lo apre nell'editor di sistema.

```toml
theme = "auto"            # auto | omarchy | dark | light
font = "JetBrains Mono"   # qualsiasi font monospace installato
font_size = 13            # px dell'interfaccia; il corpo dei messaggi è +1
sync_months = 48          # mesi di posta da tenere in sync (per data di arrivo); 0 = tutta
signature = """
Simone Silvetti
+39 333 1234567
"""                       # firma per tutti gli account (facoltativa)

[[accounts]]
name = "Lavoro"           # nome mostrato e usato come mittente
type = "imap"
email = "nome@dominio.it"
imap_host = "imap.dominio.it"
imap_port = 993           # 993 = TLS implicito, altre porte = STARTTLS
smtp_host = "smtp.dominio.it"
smtp_port = 465           # 465 = TLS implicito, altre porte = STARTTLS
username = "nome@dominio.it"   # facoltativo: di default è l'email
# imap_tls = "starttls"   # solo per porte non standard: "implicit" o "starttls"
# smtp_tls = "implicit"
# sync_months = 12        # facoltativo: sostituisce quello generale per questo account
# signature = "Ufficio"   # facoltativo: firma solo per questo account ("" = nessuna)
# la password sta nel keyring di sistema, mai nel file

[[accounts]]
name = "Gmail"
type = "google"
email = "nome@gmail.com"
client_id = "1234567890-abc.apps.googleusercontent.com"
client_secret = "GOCSPX-…"
# host e porte di Gmail sono già noti; il refresh token sta nel keyring
```

La firma viene messa in fondo a ogni messaggio nuovo, risposta e inoltro, dopo la riga `-- ` (il separatore standard che molti client riconoscono); il cursore parte sopra la firma, che resta modificabile o cancellabile nel singolo messaggio. Le stringhe su più righe vanno tra `"""`.

`sync_months` limita la sync ai messaggi arrivati negli ultimi N mesi, in tutte le cartelle: quelli più vecchi non vengono scaricati (nemmeno gli header) e restano solo sul server. Con il passare dei mesi la finestra avanza e i messaggi che ne escono spariscono dal database locale, non dal server. Se cambi il valore, l'account si risincronizza subito: allargando la finestra arrivano anche i messaggi più vecchi.

I segreti stanno nel keyring con la chiave `lettera/<email>` (servizio `lettera`, utente `<email>`). Il database locale è `~/.local/share/lettera/lettera.db`.

Variabili d'ambiente utili:

- `LETTERA_HOME=/una/cartella` — config e database in una sola cartella (per provare senza toccare la tua configurazione);
- `LETTERA_CA_FILE=/percorso/ca.pem` — certificati CA aggiuntivi (server con CA privata o self-signed);
- `RUST_LOG=lettera_core=debug` — log più dettagliati (mai password né token).

## Gmail senza OAuth: password per le app

Gmail rifiuta la password normale dell'account via IMAP (`Application-specific password required`). L'alternativa più rapida al client OAuth:

1. attiva la verifica in due passaggi sull'account Google;
2. crea una password per le app su <https://myaccount.google.com/apppasswords>;
3. aggiungi l'account come `imap` con `imap.gmail.com` porta 993 e `smtp.gmail.com` porta 465, usando quella password di 16 lettere (gli spazi non contano).

Con `imap_host = "imap.gmail.com"` lettera applica comunque la semantica di Gmail (archivio che toglie l'etichetta, niente copia doppia in *Posta inviata*), come per `type = "google"`.

## Gmail: creare il client OAuth

lettera accede a Gmail con OAuth2 usando un client che crei tu nel tuo progetto Google Cloud. È gratuito e serve una volta sola.

1. Apri <https://console.cloud.google.com/> e crea un progetto (menu in alto → *Nuovo progetto*, per esempio “lettera”).
2. In *API e servizi → Libreria* cerca **Gmail API** e premi *Abilita*.
3. Vai in *Google Auth Platform* (o *API e servizi → Schermata consenso OAuth*) e premi *Inizia*:
   - nome dell'app: `lettera`; email di assistenza: la tua;
   - pubblico: **Esterno**;
   - contatto: la tua email; accetta e crea.
4. In *Pubblico* (o *Utenti di prova*) aggiungi il tuo indirizzo Gmail come **utente di prova**. Lascia l'app in modalità **Testing**: per uso personale non serve alcuna verifica.
5. In *Accesso ai dati* (o *Ambiti*) premi *Aggiungi o rimuovi ambiti* e aggiungi a mano `https://mail.google.com/`.
6. In *Client* premi *Crea client*, tipo di applicazione **App desktop**, nome `lettera`. Copia **ID client** e **client secret**.
7. In lettera: `aggiungi account` → `google` → nome, email, client id, secret → `⏎ accedi con google`. Si apre il browser: scegli l'account, accetta l'avviso “app non verificata” (è la tua) e il consenso. Torna in lettera: la sincronizzazione parte da sola.

Note:

- con l'app in modalità *Testing* Google fa scadere il refresh token dopo **7 giorni**: quando succede la status line mostra un errore di accesso; ripeti `aggiungi account` con la stessa email per riautorizzare (l'account non viene duplicato). Per evitarlo, in *Pubblico* premi *Pubblica app*: resta non verificata (vedrai l'avviso al login) ma il token non scade;
- per account Google Workspace l'amministratore deve consentire l'accesso IMAP e le app OAuth di terze parti;
- archiviare su Gmail toglie il messaggio dalla INBOX e lo lascia in *Tutti i messaggi*; eliminare lo sposta nel *Cestino*. Funziona con le impostazioni IMAP predefinite di Gmail (“quando un messaggio viene eliminato dall'ultima cartella IMAP visibile: archivialo”).

## Scorciatoie

| tasto | azione |
|---|---|
| `j` / `k`, `↓` / `↑` | naviga nella lista (o tra i messaggi del thread aperto) |
| `⏎` | apri il thread · espandi/comprimi il messaggio |
| `esc` | chiudi / indietro / annulla la ricerca |
| `e` | archivia |
| `#` | elimina |
| `u` | segna come non letto |
| `s` | aggiungi/togli flag |
| `r` / `a` / `f` | rispondi / rispondi a tutti / inoltra |
| `c` | scrivi |
| `/` | cerca (la lista si filtra mentre scrivi, ricerca full-text) |
| `:` | palette dei comandi |
| `v` | originale HTML ↔ modalità lettura |
| `o` | salva l'allegato del messaggio |
| `g i` `g a` `g s` `g t` `g d` | vai a inbox, archivio, inviati, cestino, bozze |
| `g g` / `G` | inizio / fine lista |
| `n` / `p` | thread successivo / precedente (dentro un thread aperto) |
| `⌃1` … `⌃9` | cambia account |
| `R` | sincronizza ora |

In composizione: `⌃⏎` invia · `⌃a` allega (clic sul nome di un allegato per toglierlo) · `⌃b` mostra il campo ccn · `⌃q` mostra la citazione · `esc` chiude (con modifiche non salvate chiede un secondo `esc` e scarta la bozza). Le bozze si salvano da sole ogni 5 secondi e si riprendono ripremendo `c` (o `r`/`a`/`f` sullo stesso messaggio).

Nessuna scorciatoia scatta mentre scrivi in un campo, tranne `esc` e le combinazioni con `⌃`. Tutti i comandi, anche quelli senza tasto (`sposta in…`, `vai a cartella…`, `cambia account…`, `apri config`, `abilita immagini`, `info`…), sono nella palette.

Tutti i suggerimenti con un tasto (la barra in basso, i comandi sotto un messaggio, gli allegati) sono anche cliccabili: il clic fa la stessa cosa della scorciatoia. Quelli non disponibili in quel momento sono attenuati.

## Il tema di Omarchy

Con `theme = "auto"` (o `"omarchy"`) lettera legge `~/.local/state/omarchy/current/theme/colors.toml` (o `~/.config/omarchy/current/theme/colors.toml`) e ricava otto colori: `background`, `foreground`, `accent`, `red`, `green` dal file; `muted` dalla chiave `muted`/`comment`/`dim` se c'è, altrimenti un mix 55% testo/sfondo; bordi e superfici come mix al 12% e al 6%. Osserva la cartella del tema: quando cambi tema in Omarchy l'app si ricolora senza riavvio. Un file mancante o rotto non blocca nulla: lettera usa la palette scura o chiara incorporata seguendo il tema del sistema. `theme = "dark"` o `"light"` forzano le palette incorporate.

## Come funziona

- `crates/core` è il motore, senza dipendenze da Tauri: config, keyring, IMAP (`async-imap`), SMTP (`lettre`), OAuth, parsing (`mail-parser`), SQLite con FTS5, sync, threading, tema. Si testa da solo con `cargo test -p lettera-core`.
- `src-tauri` è un adattatore sottile: comandi Tauri ed eventi.
- `src/` è l'interfaccia Svelte 5, senza logica di dominio: chiede dati, disegna, manda comandi.

Ogni account usa tre connessioni IMAP: una per la sincronizzazione in background (ogni 5 minuti e su richiesta), una per le azioni e i corpi su richiesta, una in IDLE sulla INBOX con riconnessione automatica. Le azioni sono ottimistiche: la lista cambia subito, il server si aggiorna subito dopo; se qualcosa va storto la cartella viene risincronizzata.

Test: `just test` esegue test unitari, parsing su 14 fixture `.eml`, threading, sync con un server IMAP finto (anche il cambio di `UIDVALIDITY`) e i test del frontend. `just test-integration` avvia Dovecot e Mailpit in Docker (`docker-compose.test.yml`) e prova l'intero motore contro server veri: login, sync, IDLE, risposta con `In-Reply-To`/`References`, copia in Inviati, archivio, cestino.

## Limiti noti

- Solo IMAP/SMTP e Gmail; niente JMAP, Exchange/EWS, POP3, PGP, calendario, contatti, filtri.
- Si scrive solo in testo semplice; l'inoltro cita il testo dell'originale ma non ne riporta gli allegati.
- Le bozze restano locali (non vengono salvate nella cartella Bozze del server).
- La sync copre tutte le cartelle, incluso *Tutti i messaggi* su Gmail, entro la finestra di `sync_months` (48 mesi di default); la posta più vecchia non è consultabile dall'app. Nessun uso di CONDSTORE/QRESYNC.
- La ricerca vale per la cartella corrente e trova nel testo solo i messaggi di cui è già stato scaricato il corpo (prefetch automatico degli ultimi 30 giorni di INBOX).
- I nomi di cartella non ASCII in UTF-7 modificato vengono mostrati così come arrivano.
- Le immagini inline `cid:` dell'originale HTML non vengono mostrate.
- Scorciatoie non configurabili; un'unica finestra.
- Testato su Linux (Omarchy/Hyprland); su macOS compila senza codice specifico ma non è stato provato.

## Licenza

MIT, vedi [LICENSE](LICENSE). Il font Special Elite in `assets/fonts/` è distribuito con la sua licenza Apache 2.0.
