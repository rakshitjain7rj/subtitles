//! Speech to text. The rest of the app only sees `Transcriber`, so the
//! provider can be swapped without touching the pipeline.

use std::future::Future;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

pub mod scribe;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    pub language: Option<String>,
    pub words: Vec<Word>,
}

pub trait Transcriber {
    /// Transcribes an audio file into words with start and end times.
    fn transcribe(&self, audio: &Path) -> impl Future<Output = Result<Transcript>> + Send;
}
