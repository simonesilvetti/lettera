-- list_threads groups a folder's messages by thread: with this index it reads
-- them in thread order without sorting (13k-message INBOX: ~150 ms -> ~40 ms)
CREATE INDEX messages_by_folder_thread ON messages (folder_id, thread_id, date);
