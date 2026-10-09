-- Issue #46: AI relationship rows retain the persistent embedding space and
-- the uncalibrated raw-cosine score used to discover them. Link rows remain
-- space-less and keep their existing evidence shape.
ALTER TABLE relationships ADD COLUMN space_fingerprint TEXT REFERENCES embedding_spaces(id) ON DELETE CASCADE;
ALTER TABLE relationships ADD COLUMN score REAL CHECK(score IS NULL OR (score >= 0 AND score <= 1));
CREATE INDEX relationships_space_idx ON relationships(space_fingerprint);
