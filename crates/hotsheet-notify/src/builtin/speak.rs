//! `speak`: local text-to-speech through the platform's speech command (macOS `say`,
//! Linux `spd-say`/`espeak`, Windows System.Speech). Text goes in argv or stdin, never
//! through a shell.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use crate::capability::{
    AttachmentLimits, BodyFormat, Capabilities, NotifierDescriptor, NotifierSource, SettingField,
    SettingKind,
};
use crate::notifier::{CommandRunner, DeliveryError, LiveRunner, Notifier, tail};
use crate::render::Rendered;
use crate::settings::Resolved;

/// Program, arguments, and optional stdin for one utterance.
type SpeechCommand = (String, Vec<String>, Option<Vec<u8>>);

pub struct Speak {
    descriptor: NotifierDescriptor,
    runner: Arc<dyn CommandRunner>,
    platform: &'static str,
}

impl Speak {
    pub fn live() -> Speak {
        Speak::with_runner(Arc::new(LiveRunner), std::env::consts::OS)
    }

    pub fn with_runner(runner: Arc<dyn CommandRunner>, platform: &'static str) -> Speak {
        Speak {
            descriptor: descriptor(),
            runner,
            platform,
        }
    }

    /// The program and arguments for `text`, plus stdin when the text is piped.
    fn command(&self, text: &str, settings: &Resolved) -> Result<SpeechCommand, DeliveryError> {
        let voice = settings.str("voice");
        let rate = settings.number("rate").map(|r| r.round() as i64);
        let custom = settings.list("command");
        if let Some((program, args)) = custom.split_first() {
            let mut args = args.to_vec();
            args.push(text.to_string());
            return Ok((program.clone(), args, None));
        }
        match self.platform {
            "macos" => {
                let mut args = Vec::new();
                if let Some(voice) = voice {
                    args.extend(["-v".into(), voice.into()]);
                }
                if let Some(rate) = rate {
                    args.extend(["-r".into(), rate.to_string()]);
                }
                args.extend(["--".into(), text.into()]);
                Ok(("say".into(), args, None))
            }
            "linux" | "freebsd" | "openbsd" | "netbsd" => {
                let mut args = vec!["--wait".to_string()];
                if let Some(voice) = voice {
                    args.extend(["-o".into(), voice.into()]);
                }
                if let Some(rate) = rate {
                    // spd-say rates run -100..100 around a default of 0; map words-per-minute
                    // around a typical 175 wpm.
                    args.extend(["-r".into(), ((rate - 175) / 2).clamp(-100, 100).to_string()]);
                }
                args.extend(["--".into(), text.into()]);
                Ok(("spd-say".into(), args, None))
            }
            "windows" => {
                // The text arrives on stdin; the voice is the only value placed in the
                // script, as a single-quoted PowerShell literal.
                let select = voice
                    .map(|v| format!("$s.SelectVoice('{}');", v.replace('\'', "''")))
                    .unwrap_or_default();
                let script = format!(
                    "$t=[Console]::In.ReadToEnd();Add-Type -AssemblyName System.Speech;$s=New-Object System.Speech.Synthesis.SpeechSynthesizer;{select}$s.Speak($t)"
                );
                Ok((
                    "powershell".into(),
                    vec![
                        "-NoProfile".into(),
                        "-NonInteractive".into(),
                        "-Command".into(),
                        script,
                    ],
                    Some(text.as_bytes().to_vec()),
                ))
            }
            other => Err(DeliveryError::permanent(format!(
                "speech is not supported on {other}; set the 'command' setting"
            ))),
        }
    }
}

pub fn descriptor() -> NotifierDescriptor {
    NotifierDescriptor {
        id: "speak".into(),
        display_name: "Speak Aloud".into(),
        description: "Reads notifications aloud on this machine with its text-to-speech voice.".into(),
        capabilities: Capabilities {
            formats: vec![BodyFormat::Plain],
            max_title_bytes: 0,
            max_body_bytes: 400,
            attachments: AttachmentLimits::NONE,
            priority: false,
            click_url: false,
            audio: true,
        },
        settings: vec![
            SettingField::new("voice", "Voice", SettingKind::String)
                .help("A voice name the system speech command accepts; empty uses the default voice."),
            SettingField::new("rate", "Rate (Words per Minute)", SettingKind::Number),
            SettingField::new("command", "Custom Command", SettingKind::List)
                .help("Program and arguments to run instead of the platform default; the text is appended as the last argument."),
            SettingField::new("timeout_seconds", "Timeout (Seconds)", SettingKind::Number)
                .default_value(json!(60)),
        ],
        source: NotifierSource::BuiltIn,
    }
}

impl Notifier for Speak {
    fn descriptor(&self) -> &NotifierDescriptor {
        &self.descriptor
    }

    fn deliver(
        &self,
        _channel: &str,
        rendered: &Rendered,
        settings: &Resolved,
    ) -> Result<Option<String>, DeliveryError> {
        let (program, args, stdin) = self.command(&rendered.body, settings)?;
        let timeout =
            Duration::from_secs(settings.number("timeout_seconds").unwrap_or(60.0).max(1.0) as u64);
        let output = self
            .runner
            .run(&program, &args, stdin.as_deref(), timeout)?;
        if output.success {
            Ok(None)
        } else {
            Err(DeliveryError::permanent(format!(
                "'{program}' exited with {}: {}",
                output.code.map_or("a signal".into(), |c| c.to_string()),
                tail(&output.stderr, 300)
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{EventKind, Message};
    use crate::render::render;
    use crate::testing::RecordingRunner;
    use serde_json::json;

    fn resolved(value: serde_json::Value) -> Resolved {
        Resolved(serde_json::from_value(value).unwrap())
    }

    fn rendered() -> Rendered {
        let mut m = Message::new(
            EventKind::ProjectStopped,
            "Hot Sheet project Demo has stopped working on tickets",
        );
        m.body = "No AI activity for **15 minutes**. See [details](https://x.test).".into();
        render(&m, &descriptor().capabilities)
    }

    #[test]
    fn macos_uses_say_with_voice_and_rate_in_argv() {
        let runner = Arc::new(RecordingRunner::succeeding());
        let speak = Speak::with_runner(runner.clone(), "macos");
        speak
            .deliver(
                "c",
                &rendered(),
                &resolved(json!({"voice":"Samantha","rate":190})),
            )
            .unwrap();
        let call = runner.calls().remove(0);
        assert_eq!(call.program, "say");
        assert_eq!(
            call.args,
            [
                "-v",
                "Samantha",
                "-r",
                "190",
                "--",
                "Hot Sheet project Demo has stopped working on tickets. No AI activity for 15 minutes. See details."
            ]
        );
        assert_eq!(call.stdin, None);
    }

    #[test]
    fn linux_and_windows_and_custom_commands() {
        let runner = Arc::new(RecordingRunner::succeeding());
        Speak::with_runner(runner.clone(), "linux")
            .deliver("c", &rendered(), &resolved(json!({"rate":175})))
            .unwrap();
        let call = runner.calls().remove(0);
        assert_eq!(call.program, "spd-say");
        assert_eq!(&call.args[..3], ["--wait", "-r", "0"]);

        let runner = Arc::new(RecordingRunner::succeeding());
        Speak::with_runner(runner.clone(), "windows")
            .deliver("c", &rendered(), &resolved(json!({})))
            .unwrap();
        let call = runner.calls().remove(0);
        assert_eq!(call.program, "powershell");
        assert!(call.stdin.unwrap().starts_with(b"Hot Sheet project Demo"));

        let runner = Arc::new(RecordingRunner::succeeding());
        Speak::with_runner(runner.clone(), "haiku")
            .deliver(
                "c",
                &rendered(),
                &resolved(json!({"command":["espeak","-s","150"]})),
            )
            .unwrap();
        let call = runner.calls().remove(0);
        assert_eq!(call.program, "espeak");
        assert_eq!(call.args.len(), 3);
    }

    #[test]
    fn unsupported_platform_and_failed_command_are_permanent() {
        let speak = Speak::with_runner(Arc::new(RecordingRunner::succeeding()), "haiku");
        assert!(
            !speak
                .deliver("c", &rendered(), &resolved(json!({})))
                .unwrap_err()
                .retryable
        );
        let speak = Speak::with_runner(Arc::new(RecordingRunner::failing(1, "no voice")), "macos");
        let err = speak
            .deliver("c", &rendered(), &resolved(json!({})))
            .unwrap_err();
        assert!(!err.retryable && err.message.contains("no voice"), "{err}");
    }
}
