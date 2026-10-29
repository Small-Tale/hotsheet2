//! Faithful fakes for the injectable boundaries, shared with server and CLI tests.

use std::sync::Mutex;
use std::time::Duration;

use hotsheet_sync::LockExt;

use crate::builtin::email::{EmailSender, SmtpConfig};
use crate::http::{HttpRequest, HttpResponse, HttpTransport};
use crate::notifier::{CommandRunner, DeliveryError, ProcessOutput};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunCall {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: Option<Vec<u8>>,
}

/// Records every command and returns one fixed outcome.
pub struct RecordingRunner {
    calls: Mutex<Vec<RunCall>>,
    output: ProcessOutput,
}

impl RecordingRunner {
    pub fn succeeding() -> RecordingRunner {
        RecordingRunner::with_output(ProcessOutput {
            success: true,
            code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }

    pub fn failing(code: i32, stderr: &str) -> RecordingRunner {
        RecordingRunner::with_output(ProcessOutput {
            success: false,
            code: Some(code),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        })
    }

    pub fn with_output(output: ProcessOutput) -> RecordingRunner {
        RecordingRunner {
            calls: Mutex::new(Vec::new()),
            output,
        }
    }

    pub fn calls(&self) -> Vec<RunCall> {
        self.calls.lock_or_recover().clone()
    }
}

impl CommandRunner for RecordingRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<&[u8]>,
        _timeout: Duration,
    ) -> Result<ProcessOutput, DeliveryError> {
        self.calls.lock_or_recover().push(RunCall {
            program: program.into(),
            args: args.to_vec(),
            stdin: stdin.map(<[u8]>::to_vec),
        });
        Ok(self.output.clone())
    }
}

/// Records requests and replies with queued responses (the last one repeats).
pub struct FakeHttp {
    requests: Mutex<Vec<HttpRequest>>,
    replies: Mutex<Vec<Result<HttpResponse, String>>>,
}

impl FakeHttp {
    pub fn replying(status: u16, body: &str) -> FakeHttp {
        FakeHttp::sequence(vec![Ok(HttpResponse {
            status,
            body: body.into(),
        })])
    }

    pub fn unreachable() -> FakeHttp {
        FakeHttp::sequence(vec![Err("connection refused".into())])
    }

    pub fn sequence(replies: Vec<Result<HttpResponse, String>>) -> FakeHttp {
        FakeHttp {
            requests: Mutex::new(Vec::new()),
            replies: Mutex::new(replies),
        }
    }

    pub fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock_or_recover().clone()
    }
}

impl HttpTransport for FakeHttp {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        self.requests.lock_or_recover().push(request.clone());
        let mut replies = self.replies.lock_or_recover();
        if replies.len() > 1 {
            replies.remove(0)
        } else {
            replies
                .first()
                .cloned()
                .unwrap_or_else(|| Err("no reply queued".into()))
        }
    }
}

/// Records formatted messages instead of opening SMTP connections.
#[derive(Default)]
pub struct RecordingSmtp {
    sent: Mutex<Vec<(SmtpConfig, String)>>,
    failure: Option<DeliveryError>,
}

impl RecordingSmtp {
    pub fn failing(error: DeliveryError) -> RecordingSmtp {
        RecordingSmtp {
            sent: Mutex::new(Vec::new()),
            failure: Some(error),
        }
    }

    pub fn sent(&self) -> Vec<(SmtpConfig, String)> {
        self.sent.lock_or_recover().clone()
    }
}

impl EmailSender for RecordingSmtp {
    fn send(&self, config: &SmtpConfig, email: &lettre::Message) -> Result<(), DeliveryError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        self.sent.lock_or_recover().push((
            config.clone(),
            String::from_utf8_lossy(&email.formatted()).into_owned(),
        ));
        Ok(())
    }
}
