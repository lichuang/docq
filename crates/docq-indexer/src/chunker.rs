use docq_core::{ChunkCandidate, Chunker};
use tokenizers::Tokenizer;

pub struct SentenceSplitter {
  tokenizer: Tokenizer,
  chunk_size: usize,
  chunk_overlap: usize,
}

impl SentenceSplitter {
  pub fn new(tokenizer: Tokenizer, chunk_size: usize, chunk_overlap: usize) -> Self {
    Self {
      tokenizer,
      chunk_size,
      chunk_overlap,
    }
  }

  fn token_count(&self, text: &str) -> usize {
    self.tokenizer.encode(text, false).map(|enc| enc.get_ids().len()).unwrap_or(text.chars().count())
  }

  fn split_paragraphs(text: &str) -> Vec<&str> {
    text.split("\n\n\n").filter(|p| !p.trim().is_empty()).collect()
  }

  fn split_sentences(para: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut start = 0;
    let chars: Vec<char> = para.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
      if matches!(ch, '.' | '!' | '?' | '。' | '！' | '？') {
        let end = i + 1;
        let s: String = chars[start..end].iter().collect();
        let trimmed = s.trim();
        if !trimmed.is_empty() {
          sentences.push(s);
        }
        start = end;
      }
    }

    if start < chars.len() {
      let s: String = chars[start..].iter().collect();
      if !s.trim().is_empty() {
        sentences.push(s);
      }
    }

    if sentences.is_empty() && !para.trim().is_empty() {
      vec![para.to_string()]
    } else {
      sentences
    }
  }

  fn split_words(sentence: &str) -> Vec<String> {
    sentence.split_whitespace().map(|w| w.to_string()).collect()
  }

  fn split_chars(text: &str) -> Vec<String> {
    text.chars().map(|c| c.to_string()).collect()
  }
}

impl Chunker for SentenceSplitter {
  fn chunk(&self, text: &str) -> Vec<ChunkCandidate> {
    if text.trim().is_empty() {
      return Vec::new();
    }

    let mut units: Vec<(String, usize)> = Vec::new();
    for para in Self::split_paragraphs(text) {
      for sentence in Self::split_sentences(para) {
        let st = self.token_count(&sentence);
        if st <= self.chunk_size {
          units.push((sentence, st));
        } else {
          for word in Self::split_words(&sentence) {
            let wt = self.token_count(&word);
            if wt <= self.chunk_size {
              units.push((word, wt));
            } else {
              for ch in Self::split_chars(&word) {
                units.push((ch, 1));
              }
            }
          }
        }
      }
    }

    let mut chunks: Vec<ChunkCandidate> = Vec::new();
    let mut current: Vec<(String, usize)> = Vec::new();
    let mut current_tokens = 0usize;
    let mut current_start = 0usize;

    let mut byte_pos = 0usize;
    for (unit, unit_tokens) in &units {
      let unit_bytes = unit.len();

      if current_tokens + unit_tokens > self.chunk_size && !current.is_empty() {
        let joined = current.iter().map(|(s, _)| s.as_str()).collect::<String>();
        let end = current_start + joined.len();
        chunks.push(ChunkCandidate {
          text: joined.clone(),
          byte_range: current_start..end,
        });

        let mut overlap_units: Vec<(String, usize)> = Vec::new();
        let mut overlap_tokens = 0usize;
        for (u, ut) in current.iter().rev() {
          if overlap_tokens + ut > self.chunk_overlap {
            break;
          }
          overlap_tokens += ut;
          overlap_units.insert(0, (u.clone(), *ut));
        }

        let overlap_bytes: usize = overlap_units.iter().map(|(s, _)| s.len()).sum();

        current = overlap_units;
        current_tokens = overlap_tokens;
        current_start += joined.len() - overlap_bytes;
      }

      if current.is_empty() {
        current_start = byte_pos;
      }
      current.push((unit.clone(), *unit_tokens));
      current_tokens += unit_tokens;
      byte_pos += unit_bytes;
    }

    if !current.is_empty() {
      let joined = current.iter().map(|(s, _)| s.as_str()).collect::<String>();
      let end = current_start + joined.len();
      chunks.push(ChunkCandidate {
        text: joined,
        byte_range: current_start..end,
      });
    }

    chunks
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tokenizers::Tokenizer;
  use tokenizers::models::wordlevel::WordLevelBuilder;
  use tokenizers::pre_tokenizers::whitespace::Whitespace;

  fn test_tokenizer() -> Tokenizer {
    let mut vocab = std::collections::HashMap::new();
    vocab.insert("[UNK]".to_string(), 0u32);
    vocab.insert("[PAD]".to_string(), 1u32);
    for (i, c) in ("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789").chars().enumerate() {
      vocab.insert(c.to_string(), (i + 2) as u32);
    }
    let model = WordLevelBuilder::new().vocab(vocab).unk_token("[UNK]".to_string()).build().unwrap();
    let mut tok = Tokenizer::new(model);
    tok.with_pre_tokenizer(Some(Whitespace::default()));
    tok
  }

  #[test]
  fn test_short_text_single_chunk() {
    let tok = test_tokenizer();
    let splitter = SentenceSplitter::new(tok, 100, 10);
    let text = "Hello world. This is a test.";
    let chunks = splitter.chunk(text);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].byte_range.start, 0);
    assert_eq!(chunks[0].byte_range.end, text.len());
    assert_eq!(chunks[0].text, text);
  }

  #[test]
  fn test_multiple_sentences_multiple_chunks() {
    let tok = test_tokenizer();
    let splitter = SentenceSplitter::new(tok, 3, 1);
    let text = "Hello world. Foo bar. Baz qux.";
    let chunks = splitter.chunk(text);
    assert!(chunks.len() >= 2, "expected at least 2 chunks, got {}", chunks.len());
    for ch in &chunks {
      assert!(ch.byte_range.end > ch.byte_range.start);
      assert!(!ch.text.is_empty());
    }
  }

  #[test]
  fn test_byte_ranges_cover_text() {
    let tok = test_tokenizer();
    let splitter = SentenceSplitter::new(tok, 5, 2);
    let text = "First sentence here. Second one too. Third is short.";
    let chunks = splitter.chunk(text);
    assert!(!chunks.is_empty());
    assert_eq!(chunks[0].byte_range.start, 0);
    for w in chunks.windows(2) {
      assert!(
        w[0].byte_range.end >= w[1].byte_range.start,
        "chunk end {} < next start {}",
        w[0].byte_range.end,
        w[1].byte_range.start
      );
    }
    let last = chunks.last().unwrap();
    assert!(last.byte_range.end <= text.len());
  }

  #[test]
  fn test_chinese_punctuation_split() {
    let tok = test_tokenizer();
    let splitter = SentenceSplitter::new(tok, 100, 10);
    let text = "这是一个句子。这是另一个句子！还有第三个？";
    let chunks = splitter.chunk(text);
    assert_eq!(chunks.len(), 1, "with chunk_size=100 all 3 sentences fit in one chunk");
    assert_eq!(chunks[0].text, text);
  }

  #[test]
  fn test_empty_text() {
    let tok = test_tokenizer();
    let splitter = SentenceSplitter::new(tok, 100, 10);
    let chunks = splitter.chunk("");
    assert!(chunks.is_empty());
  }

  #[test]
  fn test_paragraph_split() {
    let tok = test_tokenizer();
    let splitter = SentenceSplitter::new(tok, 100, 10);
    let text = "Paragraph one here.\n\n\nParagraph two here.";
    let chunks = splitter.chunk(text);
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].text.contains("Paragraph one"));
    assert!(chunks[0].text.contains("Paragraph two"));
  }

  #[test]
  fn test_overlap_between_chunks() {
    let tok = test_tokenizer();
    let splitter = SentenceSplitter::new(tok, 2, 1);
    let text = "alpha beta gamma delta epsilon zeta";
    let chunks = splitter.chunk(text);
    assert!(chunks.len() >= 2, "expected at least 2 chunks, got {}", chunks.len());
    if chunks.len() >= 2 {
      assert!(
        chunks[0].byte_range.end > chunks[1].byte_range.start,
        "expected overlap: first.end={} > second.start={}",
        chunks[0].byte_range.end,
        chunks[1].byte_range.start
      );
    }
  }
}
