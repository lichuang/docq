use std::path::Path;
use std::sync::{Arc, Mutex, Once};

use chrono::{DateTime, Utc};
use docq_core::{Chunk, Document, ModelSpec, Result, Storage, StoreError};
use rusqlite::ffi::sqlite3_auto_extension;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use sqlite_vec::sqlite3_vec_init;

use crate::error::map_rusqlite;

static VEC_EXT_LOADED: Once = Once::new();

fn ensure_vec_extension() {
  VEC_EXT_LOADED.call_once(|| unsafe {
    sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_vec_init as *const ())));
  });
}

fn embedding_to_bytes(vec: &[f32]) -> Vec<u8> {
  let mut bytes = Vec::with_capacity(vec.len() * 4);
  for &f in vec {
    bytes.extend_from_slice(&f.to_ne_bytes());
  }
  bytes
}

pub struct SqliteStorage {
  conn: Arc<Mutex<Connection>>,
}

impl SqliteStorage {
  pub fn open(path: impl AsRef<Path>) -> Result<Self> {
    ensure_vec_extension();
    let conn = Connection::open(path).map_err(map_rusqlite)?;
    Ok(Self {
      conn: Arc::new(Mutex::new(conn)),
    })
  }

  pub fn open_in_memory() -> Result<Self> {
    ensure_vec_extension();
    let conn = Connection::open_in_memory().map_err(map_rusqlite)?;
    Ok(Self {
      conn: Arc::new(Mutex::new(conn)),
    })
  }
}

fn poisoned() -> StoreError {
  StoreError::Other("mutex poisoned".into())
}

impl Storage for SqliteStorage {
  fn init(&self) -> Result<()> {
    let conn = self.conn.lock().map_err(|_| poisoned())?;
    conn
      .execute_batch(
        "CREATE TABLE IF NOT EXISTS documents (
         -- Indexed file; one row per .txt/.md added via `docq add`.
         -- Renaming the file changes doc_id -> reindex, keeping logic simple.
         doc_id       TEXT PRIMARY KEY,  -- file-relative path, the Document.id
         file_path    TEXT NOT NULL,     -- absolute or workspace-relative fs path
         content_hash TEXT NOT NULL,     -- SHA-256 of file bytes; drives incremental reindex
         content_size INTEGER NOT NULL,  -- byte length of the original file
         indexed_at   TEXT NOT NULL      -- RFC3339 UTC timestamp of last successful index
       );
       CREATE TABLE IF NOT EXISTS chunks (
         -- Text block produced by the Chunker; `text` is the exact string embedded.
         -- chunk_id is the SHA-256 of `text`, so identical content is stored once.
         -- P3 will add parallel `vec_chunks` (sqlite-vec) and `fts_chunks` (FTS5) tables
         -- keyed by the same chunk_id.
         chunk_id   TEXT PRIMARY KEY,  -- SHA-256 of `text`; content-addressed dedup
         doc_id     TEXT NOT NULL,     -- FK -> documents.doc_id
         text       TEXT NOT NULL,     -- full original text actually embedded
         start_byte INTEGER NOT NULL,  -- byte offset in the source file (inclusive)
         end_byte   INTEGER NOT NULL,   -- byte offset in the source file (exclusive)
         FOREIGN KEY (doc_id) REFERENCES documents(doc_id)
       );
       CREATE TABLE IF NOT EXISTS model_versions (
         -- Records which model produced the current embeddings / rerank scores / chat answers.
         -- On embedding-model upgrade the stored vectors become stale; the indexer compares
         -- the stored spec here against the live one and triggers an explicit reindex
         -- rather than serving silently-mismatched vectors.
         role     TEXT PRIMARY KEY,   -- 'embedding' / 'reranker' / 'chat'
         repo_id  TEXT NOT NULL,      -- HuggingFace repo id, e.g. 'BAAI/bge-small-zh-v1.5'
         filename TEXT NOT NULL,      -- on-disk artifact filename (.onnx / .gguf)
         revision TEXT NOT NULL,      -- HuggingFace revision or commit hash
         checksum TEXT                -- optional content checksum for verifying downloads
       );
       CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
         -- sqlite-vec virtual table; one row per embedded chunk, keyed by the same chunk_id
         -- as `chunks`. Vectors are stored as packed native-endian f32 bytes via `vec_f32`.
         -- `distance_metric=cosine` matches the design doc §10 for semantic similarity.
         chunk_id  TEXT PRIMARY KEY,
         embedding FLOAT[512] distance_metric=cosine
       );
       CREATE VIRTUAL TABLE IF NOT EXISTS fts_chunks USING fts5(
         -- FTS5 full-text index over jieba-pre-tokenised text.
         -- `text` here stores space-separated tokens (jieba output), NOT the raw chunk text.
         -- Raw text lives in `chunks.text`; this table only serves BM25 ranking.
         -- `unicode61` splits on the spaces between tokens, treating each jieba word as a term.
         chunk_id,
         text,
         tokenize='unicode61'
       );",
      )
      .map_err(map_rusqlite)?;
    Ok(())
  }

  fn add_document(&self, doc: &Document) -> Result<()> {
    let conn = self.conn.lock().map_err(|_| poisoned())?;
    conn
      .execute(
        "INSERT OR REPLACE INTO documents
         (doc_id, file_path, content_hash, content_size, indexed_at)
       VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
          doc.id,
          doc.file_path.to_string_lossy(),
          doc.content_hash,
          doc.content_size as i64,
          doc.indexed_at.to_rfc3339(),
        ],
      )
      .map_err(map_rusqlite)?;
    Ok(())
  }

  fn get_document(&self, doc_id: &str) -> Result<Option<Document>> {
    let conn = self.conn.lock().map_err(|_| poisoned())?;
    let row = conn
      .query_row(
        "SELECT doc_id, file_path, content_hash, content_size, indexed_at
       FROM documents WHERE doc_id = ?1",
        params![doc_id],
        |r| {
          Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, String>(4)?,
          ))
        },
      )
      .optional()
      .map_err(map_rusqlite)?;

    match row {
      Some((id, file_path, content_hash, content_size, ts)) => {
        let indexed_at =
          DateTime::parse_from_rfc3339(&ts).map_err(|e| StoreError::Other(e.to_string()))?.with_timezone(&Utc);
        Ok(Some(Document {
          id,
          file_path: file_path.into(),
          content_hash,
          content_size: content_size as usize,
          indexed_at,
        }))
      }
      None => Ok(None),
    }
  }

  fn list_documents(&self) -> Result<Vec<Document>> {
    let conn = self.conn.lock().map_err(|_| poisoned())?;
    let mut stmt = conn
      .prepare("SELECT doc_id, file_path, content_hash, content_size, indexed_at FROM documents")
      .map_err(map_rusqlite)?;

    let rows: Vec<(String, String, String, i64, String)> = stmt
      .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
      .map_err(map_rusqlite)?
      .collect::<rusqlite::Result<Vec<_>>>()
      .map_err(map_rusqlite)?;

    let docs = rows
      .into_iter()
      .map(|(id, file_path, content_hash, content_size, ts)| {
        let indexed_at =
          DateTime::parse_from_rfc3339(&ts).map_err(|e| StoreError::Other(e.to_string()))?.with_timezone(&Utc);
        Ok(Document {
          id,
          file_path: file_path.into(),
          content_hash,
          content_size: content_size as usize,
          indexed_at,
        })
      })
      .collect::<Result<Vec<_>>>()?;
    Ok(docs)
  }

  fn delete_document(&self, doc_id: &str) -> Result<()> {
    let conn = self.conn.lock().map_err(|_| poisoned())?;
    conn.execute("DELETE FROM documents WHERE doc_id = ?1", params![doc_id]).map_err(map_rusqlite)?;
    Ok(())
  }

  fn add_chunks(&self, chunks: &[Chunk]) -> Result<()> {
    let mut conn = self.conn.lock().map_err(|_| poisoned())?;
    let tx = conn.transaction().map_err(map_rusqlite)?;
    {
      let mut stmt = tx
        .prepare(
          "INSERT OR REPLACE INTO chunks (chunk_id, doc_id, text, start_byte, end_byte)
           VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .map_err(map_rusqlite)?;
      for chunk in chunks {
        stmt
          .execute(params![
            chunk.id,
            chunk.doc_id,
            chunk.text,
            chunk.byte_range.start as i64,
            chunk.byte_range.end as i64,
          ])
          .map_err(map_rusqlite)?;
      }
    }
    tx.commit().map_err(map_rusqlite)?;
    Ok(())
  }

  fn get_chunks(&self, chunk_ids: &[String]) -> Result<Vec<Chunk>> {
    if chunk_ids.is_empty() {
      return Ok(Vec::new());
    }
    let conn = self.conn.lock().map_err(|_| poisoned())?;
    let placeholders = (0..chunk_ids.len()).map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
      "SELECT chunk_id, doc_id, text, start_byte, end_byte FROM chunks WHERE chunk_id IN ({})",
      placeholders
    );
    let mut stmt = conn.prepare(&sql).map_err(map_rusqlite)?;
    let rows = stmt
      .query_map(params_from_iter(chunk_ids.iter()), |r| {
        Ok((
          r.get::<_, String>(0)?,
          r.get::<_, String>(1)?,
          r.get::<_, String>(2)?,
          r.get::<_, i64>(3)?,
          r.get::<_, i64>(4)?,
        ))
      })
      .map_err(map_rusqlite)?
      .collect::<rusqlite::Result<Vec<_>>>()
      .map_err(map_rusqlite)?;

    let chunks = rows
      .into_iter()
      .map(|(id, doc_id, text, start, end)| Chunk {
        id,
        doc_id,
        text,
        byte_range: start as usize..end as usize,
      })
      .collect();
    Ok(chunks)
  }

  fn delete_chunks_by_doc(&self, doc_id: &str) -> Result<()> {
    let conn = self.conn.lock().map_err(|_| poisoned())?;
    conn.execute("DELETE FROM chunks WHERE doc_id = ?1", params![doc_id]).map_err(map_rusqlite)?;
    Ok(())
  }

  fn add_vectors(&self, chunk_ids: &[String], embeddings: &[Vec<f32>]) -> Result<()> {
    if chunk_ids.len() != embeddings.len() {
      return Err(StoreError::Other("chunk_ids and embeddings length mismatch".into()).into());
    }
    let conn = self.conn.lock().map_err(|_| poisoned())?;
let mut stmt = conn
      .prepare("INSERT OR REPLACE INTO vec_chunks (chunk_id, embedding) VALUES (?1, ?2)")
      .map_err(map_rusqlite)?;
    for (id, emb) in chunk_ids.iter().zip(embeddings.iter()) {
      let bytes = embedding_to_bytes(emb);
      stmt.execute(params![id, bytes]).map_err(map_rusqlite)?;
    }
    Ok(())
  }

  fn search_vectors(&self, embedding: &[f32], top_k: usize) -> Result<Vec<(String, f32)>> {
    let conn = self.conn.lock().map_err(|_| poisoned())?;
    let bytes = embedding_to_bytes(embedding);
    let mut stmt = conn
      .prepare(
        "SELECT chunk_id, distance
         FROM vec_chunks
         WHERE embedding MATCH ?1 AND k = ?2
         ORDER BY distance",
      )
      .map_err(map_rusqlite)?;
    let rows = stmt
      .query_map(params![bytes, top_k as i64], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f32>(1)?))
      })
      .map_err(map_rusqlite)?
      .collect::<rusqlite::Result<Vec<_>>>()
      .map_err(map_rusqlite)?;
    Ok(rows)
  }

  fn add_fts_chunks(&self, chunk_ids: &[String], tokenized_texts: &[String]) -> Result<()> {
    if chunk_ids.len() != tokenized_texts.len() {
      return Err(StoreError::Other("chunk_ids and tokenized_texts length mismatch".into()).into());
    }
    let conn = self.conn.lock().map_err(|_| poisoned())?;
    let mut stmt = conn.prepare("INSERT OR REPLACE INTO fts_chunks (chunk_id, text) VALUES (?1, ?2)").map_err(map_rusqlite)?;
    for (id, text) in chunk_ids.iter().zip(tokenized_texts.iter()) {
      stmt.execute(params![id, text]).map_err(map_rusqlite)?;
    }
    Ok(())
  }

  fn search_text(&self, query: &str, top_k: usize) -> Result<Vec<(String, f32)>> {
    let conn = self.conn.lock().map_err(|_| poisoned())?;
    let mut stmt = conn
      .prepare(
        "SELECT chunk_id, bm25(fts_chunks) AS score
         FROM fts_chunks
         WHERE fts_chunks MATCH ?1
         ORDER BY rank
         LIMIT ?2",
      )
      .map_err(map_rusqlite)?;
    let rows = stmt
      .query_map(params![query, top_k as i64], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f32>(1)?))
      })
      .map_err(map_rusqlite)?
      .collect::<rusqlite::Result<Vec<_>>>()
      .map_err(map_rusqlite)?;
    Ok(rows)
  }

  fn set_model_version(&self, role: &str, version: &ModelSpec) -> Result<()> {
    let conn = self.conn.lock().map_err(|_| poisoned())?;
    conn
      .execute(
        "INSERT OR REPLACE INTO model_versions (role, repo_id, filename, revision, checksum)
       VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
          role,
          version.repo_id,
          version.filename,
          version.revision,
          version.checksum
        ],
      )
      .map_err(map_rusqlite)?;
    Ok(())
  }

  fn get_model_version(&self, role: &str) -> Result<Option<ModelSpec>> {
    let conn = self.conn.lock().map_err(|_| poisoned())?;
    let row = conn
      .query_row(
        "SELECT repo_id, filename, revision, checksum FROM model_versions WHERE role = ?1",
        params![role],
        |r| {
          Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
          ))
        },
      )
      .optional()
      .map_err(map_rusqlite)?;

    match row {
      Some((repo_id, filename, revision, checksum)) => Ok(Some(ModelSpec {
        role: role.to_string(),
        repo_id,
        filename,
        revision,
        checksum,
      })),
      None => Ok(None),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use chrono::Utc;
  use docq_core::{Chunk, Document, Storage};

  fn make_doc(id: &str, content: &str) -> Document {
    Document {
      id: id.to_string(),
      file_path: format!("/tmp/{id}").into(),
      content_hash: format!("hash-{content}"),
      content_size: content.len(),
      indexed_at: Utc::now(),
    }
  }

  fn make_chunk(id: &str, doc_id: &str, text: &str, start: usize, end: usize) -> Chunk {
    Chunk {
      id: id.to_string(),
      doc_id: doc_id.to_string(),
      text: text.to_string(),
      byte_range: start..end,
    }
  }

  #[test]
  fn test_document_crud() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    storage.init().unwrap();

    let doc1 = make_doc("doc1.txt", "hello world");
    let doc2 = make_doc("doc2.txt", "foo bar");
    storage.add_document(&doc1).unwrap();
    storage.add_document(&doc2).unwrap();

    let got = storage.get_document("doc1.txt").unwrap();
    assert!(got.is_some());
    assert_eq!(got.unwrap().content_hash, "hash-hello world");

    let list = storage.list_documents().unwrap();
    assert_eq!(list.len(), 2);

    storage.delete_document("doc1.txt").unwrap();
    assert!(storage.get_document("doc1.txt").unwrap().is_none());
    assert_eq!(storage.list_documents().unwrap().len(), 1);
  }

  #[test]
  fn test_chunk_crud() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    storage.init().unwrap();

    let doc = make_doc("doc1.txt", "hello world");
    storage.add_document(&doc).unwrap();

    let c1 = make_chunk("c1", "doc1.txt", "hello", 0, 5);
    let c2 = make_chunk("c2", "doc1.txt", "world", 6, 11);
    storage.add_chunks(&[c1, c2]).unwrap();

    let got = storage.get_chunks(&["c1".to_string(), "c2".to_string()]).unwrap();
    assert_eq!(got.len(), 2);

    storage.delete_chunks_by_doc("doc1.txt").unwrap();
    let gone = storage.get_chunks(&["c1".to_string()]).unwrap();
    assert!(gone.is_empty());
  }

  #[test]
  fn test_model_version() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    storage.init().unwrap();

    assert!(storage.get_model_version("embedding").unwrap().is_none());

    let spec = ModelSpec {
      role: "embedding".into(),
      repo_id: "BAAI/bge-small-zh-v1.5".into(),
      filename: "model.onnx".into(),
      revision: "main".into(),
      checksum: Some("abc123".into()),
    };
    storage.set_model_version("embedding", &spec).unwrap();

    let got = storage.get_model_version("embedding").unwrap().unwrap();
    assert_eq!(got.repo_id, "BAAI/bge-small-zh-v1.5");
    assert_eq!(got.checksum.as_deref(), Some("abc123"));
  }

  #[test]
  fn test_vector_search() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    storage.init().unwrap();

    let doc = make_doc("doc1.txt", "content");
    storage.add_document(&doc).unwrap();

    let chunk = make_chunk("c0", "doc1.txt", "text", 0, 4);
    storage.add_chunks(&[chunk]).unwrap();

    let base = vec![0.0_f32; 512];
    let mut vectors: Vec<Vec<f32>> = Vec::new();
    let mut ids: Vec<String> = Vec::new();
    for i in 0..10 {
      let mut v = base.clone();
      v[i] = 1.0;
      vectors.push(v);
      ids.push(format!("c{i}"));
      let ch = make_chunk(&format!("c{i}"), "doc1.txt", "text", i, i + 1);
      storage.add_chunks(&[ch]).unwrap();
    }
    storage.add_vectors(&ids, &vectors).unwrap();

    let mut query = vec![0.0_f32; 512];
    query[3] = 1.0;
    let hits = storage.search_vectors(&query, 3).unwrap();
    assert_eq!(hits.len(), 3);
    assert_eq!(hits[0].0, "c3");
    assert!(hits[0].1 < hits[1].1);
  }

  #[test]
  fn test_text_search() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    storage.init().unwrap();

    let doc = make_doc("doc1.txt", "content");
    storage.add_document(&doc).unwrap();

    let chunks = vec![
      ("c1", "分布式 共识 算法 解决 多个 节点 达成 一致 的 问题"),
      ("c2", "Raft 是 一种 易于 理解 的 共识 算法"),
      ("c3", "今天 天气 不错"),
    ];
    for (id, text, _) in chunks.iter().map(|(id, text)| (id, text, 0)) {
      let ch = make_chunk(id, "doc1.txt", text, 0, text.len());
      storage.add_chunks(&[ch]).unwrap();
      storage.add_fts_chunks(&[id.to_string()], &[text.to_string()]).unwrap();
    }

    let hits = storage.search_text("共识 算法", 10).unwrap();
    assert_eq!(hits.len(), 2);
    let ids: Vec<&str> = hits.iter().map(|(id, _)| id.as_str()).collect();
    assert!(ids.contains(&"c1"));
    assert!(ids.contains(&"c2"));
    assert!(!ids.contains(&"c3"));
  }
}
