# Decisioni

Scelte non ovvie, una riga ciascuna. Quando una scelta era ambigua è stata presa l'opzione più semplice.

## Ambiente e dipendenze

- Rust installato con rustup in `~/.cargo` senza toccare il PATH; `just` e `tauri-cli` con `cargo install`.
- TypeScript 6 invece di 7: svelte-check non supporta ancora TS 7 senza installarne due versioni.
- `keyring` 4 con la feature `v1`: Secret Service (zbus) su Linux, Portachiavi su macOS; servizio `lettera`, utente = email.
- Le operazioni del keyring (bloccanti) girano in `spawn_blocking`.
- Il frontend dipende solo da `@tauri-apps/api`: dialog dei file, apertura link e notifiche sono fatti in Rust.
- `oauth2` usa la sua `reqwest` riesportata, per non avere due versioni di reqwest.
- `mail-parser` con `full_encoding` per i charset non UTF-8 meno comuni.
- Test del frontend con il runner integrato di Node (type stripping nativo): nessuna dipendenza di test.

## Rete e sicurezza

- Modalità TLS dedotta dalla porta (993/465 implicito, le altre STARTTLS); `imap_tls`/`smtp_tls` nel config per le porte non standard.
- Connessione, TLS, saluto e login IMAP hanno un timeout complessivo di 45 s: una modalità TLS sbagliata diventa un errore leggibile, non un blocco.
- Radici TLS di Mozilla (`webpki-roots`), più le CA in PEM indicate da `LETTERA_CA_FILE` (server con CA privata, test).
- `LETTERA_HOME` mette config e database nella stessa cartella (prove e test).
- Prima di salvare un account IMAP si fa un login reale: una password sbagliata non arriva né nel config né nel keyring.
- Link aperti nel browser di sistema solo se `http`, `https` o `mailto`.
- Originale HTML: ammonia al salvataggio (stili inline e attributi presentazionali ammessi), immagini remote tolte e CSP `default-src 'none'` alla visualizzazione.
- iframe `sandbox="allow-same-origin"` senza `allow-scripts`: nessuno script gira, ma il frontend intercetta i click sui link e li manda al browser.
- `:immagini` vale solo per quel messaggio e solo per la sessione corrente.
- Tauri non aggiunge nonce a `style-src` (`dangerousDisableAssetCspModification: ["style-src"]`): con un nonce il browser ignorerebbe `'unsafe-inline'` e bloccherebbe gli stili inline (lista virtualizzata, HTML delle email). Gli script restano protetti dai nonce.

## Sync

- Tre connessioni per account: sync in background, azioni e corpi in primo piano, IDLE su INBOX.
- Un solo `UID FETCH 1:* (FLAGS)` per cartella dà insieme UID nuovi, flag aggiornati e messaggi spariti; i nuovi si scaricano dal più recente, a blocchi di 200.
- Sono nuovi tutti gli UID che mancano in locale (non solo quelli ≥ UIDNEXT): una prima sync interrotta si completa da sola.
- Si sincronizzano tutte le cartelle selezionabili, anche *Tutti i messaggi* su Gmail; solo envelope.
- Finestra di sync `sync_months` (default 48, 0 = tutto, sovrascrivibile per account): `UID SEARCH SINCE` per data di arrivo, poi flag da `<UID minimo>:*`; i messaggi usciti dalla finestra si cancellano solo in locale.
- Corpi su richiesta più prefetch degli ultimi 30 giorni di INBOX, massimo 150 per giro.
- `has_attachments` è stimato dall'header (`multipart/mixed`) finché non arriva il corpo, poi corretto dal parsing; niente BODYSTRUCTURE.
- Gli allegati non sono salvati su disco: il messaggio si riscarica quando l'utente salva un allegato.
- Azioni ottimistiche: SQLite cambia subito, il server dopo; gli UID in movimento sono esclusi dalla sync per non farli ricomparire.
- Se un'azione fallisce sul server si risincronizza l'account: il server è la verità e la lista torna coerente.
- Dopo ogni azione si risincronizzano le cartelle toccate (sorgente e destinazione).
- Le richieste di sync in coda vengono unite in una sola.
- Errori di sync: nuovo tentativo con attesa crescente (30 s … 5 min); IDLE si riconnette da 2 s a 5 min.
- Server senza IDLE: NOOP e sync della INBOX ogni 60 s.
- Un ruolo (sent, trash, …) va a una sola cartella; SPECIAL-USE vince sui nomi.
- Archivio IMAP generico: cartella con ruolo `archive`, oppure `Archive` creata al primo uso.
- Eliminare: sposta nel cestino (`Trash` creato se manca); dal cestino la cancellazione è definitiva.
- Gmail: archiviare = `\Deleted` + `UID EXPUNGE` nella cartella; da Spam o Cestino invece sposta in *Tutti i messaggi*, perché lì l'expunge cancellerebbe per sempre.
- Semantica Gmail anche per account `imap` su `imap.gmail.com` (password per le app); l'errore "Application-specific password required" diventa un messaggio con le istruzioni.
- Gmail: dopo l'invio non si fa APPEND (lo fa Gmail) ma si risincronizza *Posta inviata*.

## Dati e threading

- `thread_id` è la stringa `<account_id>:<message-id della radice>`; unico tra account, quindi `get_thread` vuole solo l'id.
- Le risposte arrivate prima del padre vengono "adottate" nel thread del padre al suo arrivo.
- Messaggi senza Message-ID: id sintetico stabile (FNV-1a di data, mittente e oggetto).
- Il thread mostra i messaggi di tutte le cartelle dell'account (per esempio le risposte in Inviati), una volta sola per Message-ID.
- Indirizzi salvati come JSON `{name, addr}` per non perdere nomi con virgole.
- Ricerca FTS5 nella cartella corrente: ogni parola come prefisso, in AND.
- Gruppi per data calcolati in Rust in ora locale: oggi, ieri, ultimi 7 giorni, prima.
- Migrazioni SQL numerate, applicate con `PRAGMA user_version`; database in WAL.
- Il config viene modificato come testo (aggiunta o rimozione del blocco `[[accounts]]`) per non perdere i commenti dell'utente.
- Un account tolto dal config viene fermato e cancellato dal database al ricaricamento.

## Composizione

- Bozze solo locali in SQLite; una per (account, tipo, messaggio di partenza): `c`, `r`, `a`, `f` riprendono quella esistente.
- La citazione è separata dal corpo, nascosta sotto `⌃q` e sempre aggiunta in fondo all'invio.
- L'inoltro cita il testo originale ma non ne riporta gli allegati.
- `esc`: se tutto è salvato chiude e la bozza resta; con modifiche non salvate chiede un secondo `esc` che scarta la bozza.
- `send` aspetta la consegna SMTP (eccezione voluta alla regola dei 50 ms) per mostrare subito un eventuale errore.
- I destinatari in Ccn finiscono solo nella busta SMTP, mai negli header.
- Message-ID generato con il dominio del mittente.

## Interfaccia

- Comandi e scorciatoie da un unico registro; le scorciatoie hanno uno *scope* (lista, thread, posta, composizione, palette, ricerca, globale).
- Comandi Tauri con argomenti in snake_case (`rename_all`), come nella specifica.
- Comandi in più rispetto alla lista: `prepare_draft`, `discard_draft`, `pick_files`, `open_url`, `enable_images`, `suggest_contacts`, `app_info`; eventi in più: `body:ready`, `accounts:changed`.
- Gli hint con un tasto sono anche pulsanti che eseguono il comando del registro; disattivati se il comando non è disponibile.
- Rimuovere un allegato in composizione: `×` accanto al nome oppure `⌃d` (palette con l'elenco).
- SMTP: se il TLS implicito riceve testo in chiaro (porta STARTTLS) si riprova con STARTTLS, sempre con TLS obbligatorio; l'aggiunta di un account prova anche il login SMTP e salva la modalità che funziona.
- Aprire un thread lo segna come letto; `u` lo segna non letto e chiude il thread.
- Archiviare o eliminare con il thread aperto apre il thread successivo.
- Lista virtualizzata con righe a altezza fissa (38 px), intestazioni di gruppo comprese.
- La palette mostra solo i comandi disponibili nel contesto; i tasti di navigazione non sono elencati.
- `:info` mostra wordmark, versione, licenza e percorso del config.
- Etichette italiane per le cartelle con un ruolo (inbox, inviati, bozze, cestino, archivio, spam).
- Data in lista: l'anno compare solo se diverso da quello corrente.
- Puntino di non letto quadrato: niente `border-radius`, nemmeno lì.
- Il form Google chiede client id e secret: prima che l'account esista non c'è altro posto da cui prenderli.
- OAuth con `access_type=offline`, `prompt=consent` e `login_hint`, per avere sempre un refresh token per l'email giusta.
- Il form di aggiunta propone `imap.` e `smtp.` del dominio dell'email; la modalità TLS segue la porta ma si può cambiare con un clic.
- Notifiche mandate dal processo Rust, solo per messaggi non letti arrivati via IDLE.

## Pacchetti e identità

- Flatpak su GNOME 51 con build che scarica le dipendenze (`--share=network`): pacchetto locale, non pronto per Flathub; dati nel sandbox, non condivisi con l'installazione nativa.
- Bundle `deb` e `rpm` su Linux, `app` e `dmg` su macOS; niente AppImage (scarica strumenti durante la build).
- Icona disegnata a mano in SVG e convertita con `cargo tauri icon`; set ridotto alle icone desktop.
- Wordmark: Special Elite (Apache 2.0) convertito in tracciati con fontTools, colore `currentColor`.
- Test d'integrazione: Dovecot e Mailpit in Docker con una CA di test nel repository (la chiave privata della CA è stata scartata).
