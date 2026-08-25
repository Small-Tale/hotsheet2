//! Server-owned speech boundary. Provider credentials are resolved when constructing an
//! adapter and never appear in this request/response contract.
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Clone, Deserialize)]
pub struct TtsRequest {
    pub text: String,
    #[serde(default)]
    pub voice: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
}
pub struct TtsAudio {
    pub content_type: String,
    pub bytes: Vec<u8>,
}
pub trait TtsProvider: Send + Sync {
    fn id(&self) -> &str;
    fn synthesize(&self, text: &str, voice: Option<&str>) -> Result<TtsAudio, String>;
}
#[derive(Clone, Default)]
pub struct TtsProviders(pub Arc<Vec<Arc<dyn TtsProvider>>>);
impl TtsProviders {
    pub fn new(v: Vec<Arc<dyn TtsProvider>>) -> Self {
        Self(Arc::new(v))
    }
    pub fn synthesize(&self, r: &TtsRequest) -> Result<TtsAudio, String> {
        if r.text.trim().is_empty() {
            return Err("text must not be empty".into());
        }
        if r.text.len() > 10_000 {
            return Err("text exceeds 10000 bytes".into());
        }
        let p = match r.provider.as_deref() {
            Some(id) => self.0.iter().find(|p| p.id() == id),
            None => self.0.first(),
        }
        .ok_or_else(|| "no matching TTS provider is configured".to_string())?;
        p.synthesize(&r.text, r.voice.as_deref())
    }
}
