# lettera — development commands. Rust and Node must be on PATH.

set shell := ["bash", "-cu"]

# run the app in development (vite + tauri window with live reload)
dev:
    npm install --no-audit --no-fund
    cargo tauri dev

# release binary and packages (target/release/lettera, target/release/bundle/)
build:
    npm install --no-audit --no-fund
    cargo tauri build

# all tests; the ones against dovecot/mailpit run when LETTERA_IMAP_TEST is set
test:
    cargo test --workspace
    npm test

# integration tests against the servers in docker
test-integration:
    docker compose -f docker-compose.test.yml up -d
    LETTERA_IMAP_TEST=1 cargo test -p lettera-core --test dovecot --test engine_e2e

lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    npx svelte-check --tsconfig ./tsconfig.json --fail-on-warnings

fmt:
    cargo fmt --all

# flatpak bundle (lettera.flatpak); install it with: flatpak install --user lettera.flatpak
flatpak:
    flatpak run org.flatpak.Builder --user --force-clean --install-deps-from=flathub --repo=flatpak/repo flatpak/build flatpak/app.lettera.desktop.yml
    flatpak build-bundle flatpak/repo lettera.flatpak app.lettera.desktop --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo
