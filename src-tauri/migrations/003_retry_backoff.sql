-- Backoff for documents that keep failing to read (issue #28, ADR 0009). Additive to 002.
-- `retry_after` is epoch milliseconds; the signature and extractor version are those
-- of the last failure, so a changed file or a newer extractor is retried at once.
ALTER TABLE documents ADD COLUMN retry_failures INTEGER NOT NULL DEFAULT 0;
ALTER TABLE documents ADD COLUMN retry_after TEXT;
ALTER TABLE documents ADD COLUMN retry_signature TEXT;
ALTER TABLE documents ADD COLUMN retry_extractor INTEGER;
