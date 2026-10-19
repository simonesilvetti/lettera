-- html-only bodies were turned into text with table borders drawn as rows of
-- `─`: drop the stored bodies that have html, so they are read again (on
-- demand or by prefetch) with the new conversion. Local draft copies (local
-- UIDs) cannot be downloaded again and stay.
DELETE FROM bodies WHERE html IS NOT NULL
    AND message_id IN (SELECT id FROM messages WHERE uid < 4293918720)
