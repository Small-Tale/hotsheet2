//! A reference implementation of the `diagnostics` capability (docs/38 §38.11). Plugins
//! may embed it so `hotsheet-cli plugin verify` can exercise both transports; Hot Sheet's
//! own plugin-host tests drive it through a real child process.

use std::time::Duration;

use crate::serve::{CallContext, Handler};
use crate::{Payload, PluginError, v1};

/// The capability name.
pub const CAPABILITY: &str = "diagnostics";

/// The `diagnostics` methods.
pub mod method {
    pub const ECHO: &str = "diagnostics.Echo";
    pub const COUNT: &str = "diagnostics.Count";
    pub const SLEEP: &str = "diagnostics.Sleep";
    pub const FAIL: &str = "diagnostics.Fail";
    pub const EXIT: &str = "diagnostics.Exit";
    pub const ENVIRONMENT: &str = "diagnostics.Environment";
}

/// Answers every `diagnostics` method; anything else is `unsupported`.
#[derive(Clone, Copy, Debug, Default)]
pub struct DiagnosticsHandler;

impl Handler for DiagnosticsHandler {
    fn call(
        &self,
        method: &str,
        request: Payload,
        ctx: &CallContext,
    ) -> Result<Payload, PluginError> {
        match method {
            method::ECHO => {
                let request: v1::EchoRequest = request.decode()?;
                Ok(ctx.reply(&v1::EchoResponse {
                    text: request.text,
                    data: request.data,
                }))
            }
            method::COUNT => {
                let request: v1::CountRequest = request.decode()?;
                let mut sent = 0;
                for seq in 0..request.count {
                    if ctx.is_cancelled() {
                        return Err(PluginError::cancelled());
                    }
                    ctx.send_item(&v1::CountItem { seq })
                        .map_err(|e| PluginError::unavailable(e.to_string()))?;
                    sent += 1;
                }
                Ok(ctx.reply(&v1::CountResponse { sent }))
            }
            method::SLEEP => {
                let request: v1::SleepRequest = request.decode()?;
                if ctx.wait_cancelled(Duration::from_millis(request.millis)) {
                    Err(PluginError::cancelled())
                } else {
                    Ok(ctx.reply(&v1::Empty {}))
                }
            }
            method::FAIL => {
                let request: v1::FailRequest = request.decode()?;
                Err(request
                    .error
                    .unwrap_or_else(|| PluginError::internal("diagnostics.Fail without an error")))
            }
            method::EXIT => {
                let request: v1::ExitRequest = request.decode()?;
                std::process::exit(request.status);
            }
            method::ENVIRONMENT => {
                let mut names: Vec<String> = std::env::vars_os()
                    .filter_map(|(name, _)| name.into_string().ok())
                    .collect();
                names.sort();
                Ok(ctx.reply(&v1::EnvironmentResponse { names }))
            }
            other => Err(PluginError::unsupported(other)),
        }
    }
}
