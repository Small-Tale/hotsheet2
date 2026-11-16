//! Server-owned speech boundary. Provider credentials are resolved when constructing an
//! adapter and never appear in this request/response contract.
use serde::{Deserialize, Serialize};
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
/// One voice a server provider can speak with (HS2-CEZVSE). `name` is the value the provider
/// accepts as a synthesis `voice`; `lang` is a BCP 47 tag such as `en-US`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TtsVoice {
    pub name: String,
    pub lang: String,
}
pub trait TtsProvider: Send + Sync {
    fn id(&self) -> &str;
    fn synthesize(&self, text: &str, voice: Option<&str>) -> Result<TtsAudio, String>;
    /// The voices this provider offers. A provider that cannot list voices offers only its
    /// default voice, which is an empty list.
    fn voices(&self) -> Result<Vec<TtsVoice>, String> {
        Ok(Vec::new())
    }
}
/// Why a provider's voices could not be listed.
#[derive(Debug, PartialEq, Eq)]
pub enum TtsVoicesError {
    UnknownProvider,
    Unavailable(String),
}
#[derive(Clone)]
pub struct TtsProviders(pub Arc<Vec<Arc<dyn TtsProvider>>>);
impl Default for TtsProviders {
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::new(vec![Arc::new(MacSayProvider::default())])
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
    /// The voices of `provider`, or of the first configured provider when none is named.
    pub fn voices(&self, provider: Option<&str>) -> Result<Vec<TtsVoice>, TtsVoicesError> {
        let p = match provider {
            Some(id) => self.0.iter().find(|p| p.id() == id),
            None => self.0.first(),
        }
        .ok_or(TtsVoicesError::UnknownProvider)?;
        p.voices().map_err(TtsVoicesError::Unavailable)
    }
}

/// Parse the listing printed by macOS `say -v '?'` (HS2-CEZVSE). Each line is a voice name,
/// which may contain spaces and parentheses, then a locale such as `en_US`, then `#` and a
/// sample sentence. Blank or malformed lines and repeated names are skipped, and the locale is
/// returned as a BCP 47 tag (`en-US`).
pub fn parse_say_voices(output: &str) -> Vec<TtsVoice> {
    let mut seen = std::collections::BTreeSet::new();
    output
        .lines()
        .filter_map(|line| {
            let entry = line.split_once('#').map_or(line, |(entry, _)| entry).trim();
            let (name, locale) = entry.rsplit_once(char::is_whitespace)?;
            let name = name.trim();
            let plausible_locale = locale.starts_with(|c: char| c.is_ascii_alphabetic())
                && locale
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            if name.is_empty() || !plausible_locale || !seen.insert(name.to_string()) {
                return None;
            }
            Some(TtsVoice {
                name: name.to_string(),
                lang: locale.replace('_', "-"),
            })
        })
        .collect()
}

/// On macOS, synthesize on the server machine without putting narration text in a command line,
/// log, or persistent cache. The temporary AIFF is converted to browser-playable PCM WAV and
/// removed with its private directory after the bytes are read (HS2-5PSQJQ).
#[cfg(target_os = "macos")]
struct MacSayProvider {
    /// The `say` executable; tests substitute a fake command (HS2-CEZVSE).
    say: std::path::PathBuf,
}

#[cfg(target_os = "macos")]
impl Default for MacSayProvider {
    fn default() -> Self {
        Self {
            say: "/usr/bin/say".into(),
        }
    }
}

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
        let mut command = hotsheet_ticketing::git::launch(&self.say);
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

    fn voices(&self) -> Result<Vec<TtsVoice>, String> {
        use std::process::Stdio;

        let output = hotsheet_ticketing::git::launch(&self.say)
            .arg("-v")
            .arg("?")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|_| "local speech voices are unavailable")?;
        if !output.status.success() {
            return Err("local speech voices are unavailable".into());
        }
        Ok(parse_say_voices(&String::from_utf8_lossy(&output.stdout)))
    }
}

#[cfg(test)]
mod voice_tests {
    use super::*;

    fn voice(name: &str, lang: &str) -> TtsVoice {
        TtsVoice {
            name: name.into(),
            lang: lang.into(),
        }
    }

    #[test]
    fn hs2_cezvse_parses_say_voice_listing() {
        let output = [
            "Albert              en_US    # Hello! My name is Albert.",
            "Alice (Italian (Italy)) it_IT    # Ciao! Mi chiamo Alice.",
            "Alice (Italian (Italy)) it_IT    # Ciao! Mi chiamo Alice.",
            "Bad News            en_US    # Hello! # a second hash",
            "",
            "garbage",
            "Fiona               en_GB-u-sd-gbsct # Hello!",
            "Odd                 #no locale",
            "Numbered            42_XX    # not a locale",
        ]
        .join("\n");
        assert_eq!(
            parse_say_voices(&output),
            vec![
                voice("Albert", "en-US"),
                voice("Alice (Italian (Italy))", "it-IT"),
                voice("Bad News", "en-US"),
                voice("Fiona", "en-GB-u-sd-gbsct"),
            ]
        );
    }

    #[test]
    fn hs2_cezvse_empty_listing_has_no_voices() {
        assert!(parse_say_voices("").is_empty());
        assert!(parse_say_voices("   \n# only a comment\n").is_empty());
    }

    struct Listed;
    impl TtsProvider for Listed {
        fn id(&self) -> &str {
            "listed"
        }
        fn synthesize(&self, _: &str, _: Option<&str>) -> Result<TtsAudio, String> {
            Err("unused".into())
        }
        fn voices(&self) -> Result<Vec<TtsVoice>, String> {
            Ok(vec![voice("Zed", "en-US")])
        }
    }
    struct Silent;
    impl TtsProvider for Silent {
        fn id(&self) -> &str {
            "silent"
        }
        fn synthesize(&self, _: &str, _: Option<&str>) -> Result<TtsAudio, String> {
            Err("unused".into())
        }
    }

    #[test]
    fn hs2_cezvse_voices_route_to_the_named_provider() {
        let providers = TtsProviders::new(vec![Arc::new(Silent), Arc::new(Listed)]);
        assert!(Listed.synthesize("", None).is_err() && Silent.synthesize("", None).is_err());
        assert_eq!(providers.voices(None), Ok(Vec::new()));
        assert_eq!(
            providers.voices(Some("listed")),
            Ok(vec![voice("Zed", "en-US")])
        );
        assert_eq!(
            providers.voices(Some("missing")),
            Err(TtsVoicesError::UnknownProvider)
        );
        assert_eq!(
            TtsProviders::new(Vec::new()).voices(None),
            Err(TtsVoicesError::UnknownProvider)
        );
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    fn fake_say(script: &str) -> (tempfile::TempDir, MacSayProvider) {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let say = directory.path().join("say");
        std::fs::write(&say, script).unwrap();
        std::fs::set_permissions(&say, std::fs::Permissions::from_mode(0o755)).unwrap();
        (directory, MacSayProvider { say })
    }

    #[test]
    fn hs2_cezvse_mac_voices_come_from_say_listing() {
        let (_directory, provider) = fake_say(concat!(
            "#!/bin/sh\n",
            "[ \"$1\" = -v ] && [ \"$2\" = '?' ] || exit 3\n",
            "echo 'Albert              en_US    # Hello!'\n",
            "echo 'Anna (German (Germany)) de_DE    # Hallo!'\n",
        ));
        assert_eq!(
            provider.voices().unwrap(),
            vec![
                TtsVoice {
                    name: "Albert".into(),
                    lang: "en-US".into()
                },
                TtsVoice {
                    name: "Anna (German (Germany))".into(),
                    lang: "de-DE".into()
                },
            ]
        );
    }

    #[test]
    fn hs2_cezvse_failed_say_listing_is_unavailable() {
        let (_directory, provider) = fake_say("#!/bin/sh\nexit 1\n");
        assert!(provider.voices().is_err());
        let missing = MacSayProvider {
            say: "/nonexistent/say".into(),
        };
        assert!(missing.voices().is_err());
    }

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
