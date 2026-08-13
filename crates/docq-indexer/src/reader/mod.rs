pub mod txt_reader;

#[cfg(feature = "pdf")]
pub mod pdf_reader;

pub use txt_reader::TextFileReader;

#[cfg(feature = "pdf")]
pub use pdf_reader::PdfReader;
