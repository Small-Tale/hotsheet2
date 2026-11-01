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
#[derive(Clone)]
pub struct TtsProviders(pub Arc<Vec<Arc<dyn TtsProvider>>>);
impl Default for TtsProviders {
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::new(vec![Arc::new(MacSayProvider)])
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self::new(Vec::new())
        }
    }
}
impl TtsProviders {
    pub fn new(v: Vec<Arc<dyn TtsProvider>>) -> Self {
        Self(Arc::new(v))
    }
    pub fn available(&self) -> Vec<String> {
        self.0
            .iter()
            .map(|provider| provider.id().to_string())
            .collect()
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

/// On macOS, synthesize on the server machine without putting narration text in a command line,
/// log, or persistent cache. The temporary AIFF is converted to browser-playable PCM WAV and
/// removed with its private directory after the bytes are read (HS2-5PSQJQ).
#[cfg(target_os = "macos")]
struct MacSayProvider;

#[cfg(target_os = "macos")]
impl TtsProvider for MacSayProvider {
    fn id(&self) -> &str {
        "macos-say"
    }

    fn synthesize(&self, text: &str, voice: Option<&str>) -> Result<TtsAudio, String> {
        use std::io::Write;
        use std::process::Stdio;

        let directory = tempfile::tempdir().map_err(|_| "cannot prepare speech output")?;
        let aiff = directory.path().join("speech.aiff");
        let wav = directory.path().join("speech.wav");
        let mut command = hotsheet_ticketing::git::launch("/usr/bin/say");
        command.arg("-f").arg("-").arg("-o").arg(&aiff);
        if let Some(voice) = voice.filter(|voice| !voice.trim().is_empty()) {
            command.arg("-v").arg(voice);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "local speech voice is unavailable")?;
        child
            .stdin
            .take()
            .ok_or("local speech input is unavailable")?
            .write_all(text.as_bytes())
            .map_err(|_| "local speech input failed")?;
        if !child
            .wait()
            .map_err(|_| "local speech voice failed")?
            .success()
        {
            return Err("local speech voice failed".into());
        }
        if !hotsheet_ticketing::git::launch("/usr/bin/afconvert")
            .arg(&aiff)
            .arg("-o")
            .arg(&wav)
            .arg("-f")
            .arg("WAVE")
            .arg("-d")
            .arg("LEI16")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| "local speech conversion failed")?
            .success()
        {
            return Err("local speech conversion failed".into());
        }
        let bytes = std::fs::read(&wav).map_err(|_| "local speech output is unavailable")?;
        Ok(TtsAudio {
            content_type: "audio/wav".into(),
            bytes,
        })
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn hs2_5psqjq_default_mac_voice_returns_browser_playable_wav() {
        let providers = TtsProviders::default();
        assert_eq!(providers.available(), ["macos-say"]);
        let audio = providers
            .synthesize(&TtsRequest {
                text: "Synthetic speech check.".into(),
                voice: None,
                provider: Some("macos-say".into()),
            })
            .unwrap();
        assert_eq!(audio.content_type, "audio/wav");
        assert!(audio.bytes.starts_with(b"RIFF"));
    }
}
