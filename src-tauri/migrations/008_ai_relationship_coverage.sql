-- Issue #46: progressive, resumable AI relationship discovery. Coverage is
-- recorded apart from retained candidate edges so "every pair was compared" is
-- never confused with "every candidate was kept".

-- Per workspace and persistent embedding space: the monotonic admission
-- counter. It is never decremented, so a deleted or edited document's seq is
-- never reused.
CREATE TABLE ai_relationship_seq (
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  space_id     TEXT NOT NULL REFERENCES embedding_spaces(id) ON DELETE CASCADE,
  next_seq     INTEGER NOT NULL CHECK(next_seq >= 1),
  PRIMARY KEY (workspace_id, space_id)
);

-- One row per admitted document revision. A pair {X, Y} with X.seq < Y.seq is
-- compared by Y's job against X; `partner_cursor_seq` means every partner with
-- seq <= it is finished.
CREATE TABLE ai_relationship_coverage (
  workspace_id       TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  space_id           TEXT NOT NULL REFERENCES embedding_spaces(id) ON DELETE CASCADE,
  document_id        TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  content_hash       TEXT NOT NULL,
  seq                INTEGER NOT NULL CHECK(seq >= 1),
  partner_cursor_seq INTEGER NOT NULL DEFAULT 0 CHECK(partner_cursor_seq >= 0),
  candidate_overflow INTEGER NOT NULL DEFAULT 0 CHECK(candidate_overflow IN (0, 1)),
  updated_at         TEXT NOT NULL,
  PRIMARY KEY (workspace_id, space_id, document_id),
  UNIQUE (workspace_id, space_id, seq)
);

-- At most one in-flight pair per job. The accumulator is bounded: the best
-- cosine, a few chunk-ordinal references and the best shared-fact reference.
CREATE TABLE ai_pair_progress (
  workspace_id     TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  space_id         TEXT NOT NULL REFERENCES embedding_spaces(id) ON DELETE CASCADE,
  document_id      TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  partner_id       TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  document_hash    TEXT NOT NULL,
  partner_hash     TEXT NOT NULL,
  partner_seq      INTEGER NOT NULL,
  next_left        INTEGER NOT NULL CHECK(next_left >= 0),
  next_right       INTEGER NOT NULL CHECK(next_right >= 0),
  accumulator_json TEXT NOT NULL,
  PRIMARY KEY (workspace_id, space_id, document_id)
);

-- Fair round-robin pointer across jobs, so no document takes a whole run.
CREATE TABLE ai_relationship_schedule (
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  space_id     TEXT NOT NULL REFERENCES embedding_spaces(id) ON DELETE CASCADE,
  next_job_seq INTEGER NOT NULL CHECK(next_job_seq >= 0),
  PRIMARY KEY (workspace_id, space_id)
);

-- Internal ranking cosine for retained candidates; not on the wire.
ALTER TABLE relationships ADD COLUMN discovery_cosine REAL;
-- Per-endpoint candidate lookups (cap eviction, a document's reset) seek one
-- index per endpoint column; their (space, type) prefix also serves the
-- whole-space reads of the displayed set.
CREATE INDEX relationships_space_type_source_idx ON relationships(space_fingerprint, relationship_type, source_document_id);
CREATE INDEX relationships_space_type_target_idx ON relationships(space_fingerprint, relationship_type, target_document_id);
