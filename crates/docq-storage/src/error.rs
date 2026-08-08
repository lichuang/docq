use docq_core::StoreError;

pub(crate) fn map_rusqlite(e: rusqlite::Error) -> StoreError {
  StoreError::Other(e.to_string())
}
