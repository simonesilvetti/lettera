-- the raw header block of each downloaded body, for the privacy report
-- (which platform sent the message). Bodies stored before stay without it
-- until the report asks the server for their headers.
ALTER TABLE bodies ADD COLUMN headers TEXT;
