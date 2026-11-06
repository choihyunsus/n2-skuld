CREATE TABLE IF NOT EXISTS verdandi_semantic_edges (
  project TEXT NOT NULL,
  agent TEXT NOT NULL,
  source_path TEXT NOT NULL,
  target_path TEXT NOT NULL,
  similarity REAL NOT NULL CHECK(similarity >= 0 AND similarity <= 1),
  edge_type TEXT NOT NULL DEFAULT 'semantic',
  model TEXT NOT NULL,
  computed_at INTEGER NOT NULL,
  PRIMARY KEY(project,agent,source_path,target_path)
);
CREATE INDEX IF NOT EXISTS idx_verdandi_semantic_scope
  ON verdandi_semantic_edges(project,agent,source_path,similarity DESC,target_path);
