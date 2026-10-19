# lettera — comandi di sviluppo. Rust e Node devono essere nel PATH.

set shell := ["bash", "-cu"]

# avvia l'app in sviluppo (vite + finestra tauri con ricarica)
dev:
    npm install --no-audit --no-fund
    cargo tauri dev

# binario e pacchetti di release (target/release/lettera, target/release/bundle/)
build:
    npm install --no-audit --no-fund
    cargo tauri build

# tutti i test; quelli contro dovecot/mailpit girano se LETTERA_IMAP_TEST è impostata
test:
    cargo test --workspace
    npm test

# test di integrazione con i server in docker
test-integration:
    docker compose -f docker-compose.test.yml up -d
    LETTERA_IMAP_TEST=1 cargo test -p lettera-core --test dovecot --test engine_e2e

lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    npx svelte-check --tsconfig ./tsconfig.json --fail-on-warnings

fmt:
    cargo fmt --all

# pacchetto flatpak (lettera.flatpak); installalo con: flatpak install --user lettera.flatpak
flatpak:
    flatpak run org.flatpak.Builder --user --force-clean --install-deps-from=flathub --repo=flatpak/repo flatpak/build flatpak/app.lettera.desktop.yml
    flatpak build-bundle flatpak/repo lettera.flatpak app.lettera.desktop --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo
