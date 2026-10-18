CREATE TABLE accounts (
    id    INTEGER PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    name  TEXT NOT NULL,
    kind  TEXT NOT NULL
);

CREATE TABLE folders (
    id          INTEGER PRIMARY KEY,
    account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    delimiter   TEXT,
    role        TEXT,
    uidvalidity INTEGER NOT NULL DEFAULT 0,
    uidnext     INTEGER NOT NULL DEFAULT 0,
    last_sync   INTEGER,
    UNIQUE (account_id, name)
);

CREATE TABLE messages (
    id              INTEGER PRIMARY KEY,
    account_id      INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    folder_id       INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uid             INTEGER NOT NULL,
    message_id      TEXT NOT NULL,
    in_reply_to     TEXT,
    refs            TEXT,
    from_name       TEXT,
    from_addr       TEXT,
    reply_to        TEXT,
    to_addrs        TEXT,
    cc_addrs        TEXT,
    subject         TEXT NOT NULL DEFAULT '',
    subject_norm    TEXT NOT NULL DEFAULT '',
    date            INTEGER NOT NULL DEFAULT 0,
    flags           TEXT NOT NULL DEFAULT '',
    size            INTEGER NOT NULL DEFAULT 0,
    has_attachments INTEGER NOT NULL DEFAULT 0,
    snippet         TEXT NOT NULL DEFAULT '',
    thread_id       TEXT NOT NULL,
    UNIQUE (folder_id, uid)
);

CREATE INDEX messages_by_message_id ON messages (account_id, message_id);
CREATE INDEX messages_by_thread ON messages (account_id, thread_id);
CREATE INDEX messages_by_folder_date ON messages (folder_id, date DESC);
CREATE INDEX messages_by_subject ON messages (account_id, subject_norm);
CREATE INDEX messages_by_from ON messages (from_addr);

CREATE TABLE bodies (
    message_id INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    text       TEXT NOT NULL,
    html       TEXT
);

CREATE TABLE attachments (
    id         INTEGER PRIMARY KEY,
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    part_index INTEGER NOT NULL,
    filename   TEXT NOT NULL,
    mime       TEXT NOT NULL,
    size       INTEGER NOT NULL
);

CREATE INDEX attachments_by_message ON attachments (message_id);

CREATE TABLE drafts (
    id         INTEGER PRIMARY KEY,
    account    TEXT NOT NULL,
    kind       TEXT NOT NULL,
    source     INTEGER,
    data       TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE VIRTUAL TABLE messages_fts USING fts5 (sender, subject, body);

CREATE TRIGGER messages_fts_insert AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts (rowid, sender, subject, body)
    VALUES (new.id, coalesce(new.from_name, '') || ' ' || coalesce(new.from_addr, ''), new.subject, '');
END;

CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
    DELETE FROM messages_fts WHERE rowid = old.id;
END;
