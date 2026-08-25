//! Agent Client Protocol drive + standard usage mapper (HS2-DEK94W/HS2-96BZEF).

use serde_json::Value;

use crate::drive::{Drive, DriveCtx, DriveError, DriveInfo, Target, Transport, TurnHandle, Usage};

pub struct AcpDrive;

impl Drive for AcpDrive {
    fn info(&self) -> DriveInfo {
        DriveInfo {
            transport: Transport::Acp,
        }
    }

    fn supports_interrupt(&self) -> bool {
        true
    }

    fn run(
        &self,
        target: &Target,
        content: &str,
        ctx: &DriveCtx,
    ) -> Result<Box<dyn TurnHandle>, DriveError> {
        let client = ctx
            .acp
            .ok_or_else(|| DriveError::NotConnected("ACP agent is not connected".into()))?;
        client.start_turn(target.0.as_deref(), &ctx.cwd, content)
    }
}

/// Map only ACP's standard PromptResponse usage counters. Implementations are observed
/// with both protocol-style snake_case and SDK camelCase; model/cache/cost are deliberately
/// absent because the standard counters do not guarantee them.
pub fn usage(value: &Value) -> Option<Usage> {
    let u = value
        .pointer("/result/usage")
        .or_else(|| value.get("usage"))?;
    let number = |snake: &str, camel: &str| {
        u.get(snake)
            .or_else(|| u.get(camel))
            .and_then(Value::as_u64)
    };
    let tokens_in = number("input_tokens", "inputTokens")?;
    let tokens_out = number("output_tokens", "outputTokens")?;
    Some(Usage {
        model: None,
        tokens_in,
        tokens_out,
        cost_usd: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drive::{DoneReason, DriveCtx};
    use crate::ports::{AcpClient, ProcessSpawner, SpawnSpec, SpawnedProcess};
    use serde_json::json;
    use std::path::{Path, PathBuf};

    struct NoSpawn;
    impl ProcessSpawner for NoSpawn {
        fn spawn(&self, _: &SpawnSpec) -> std::io::Result<Box<dyn SpawnedProcess>> {
            panic!("ACP must use its protocol client")
        }
    }
    struct Done;
    impl TurnHandle for Done {
        fn is_busy(&mut self) -> bool {
            false
        }
        fn wait(&mut self) -> DoneReason {
            DoneReason::Completed
        }
    }
    struct FakeAcp;
    impl AcpClient for FakeAcp {
        fn start_turn(
            &self,
            _: Option<&str>,
            _: &Path,
            content: &str,
        ) -> Result<Box<dyn TurnHandle>, DriveError> {
            assert_eq!(content, "work");
            Ok(Box::new(Done))
        }
    }

    #[test]
    fn drive_delegates_to_the_injected_acp_session() {
        let ctx = DriveCtx {
            cwd: PathBuf::from("/project"),
            spawner: &NoSpawn,
            env: vec![],
            app_server: None,
            channel: None,
            acp: Some(&FakeAcp),
        };
        let mut turn = AcpDrive.run(&Target::default(), "work", &ctx).unwrap();
        assert_eq!(turn.wait(), DoneReason::Completed);
    }

    #[test]
    fn maps_only_standard_prompt_response_counters() {
        assert_eq!(
            usage(
                &json!({"result":{"usage":{"input_tokens":41,"output_tokens":7,"total_tokens":48}}})
            ),
            Some(Usage {
                model: None,
                tokens_in: 41,
                tokens_out: 7,
                cost_usd: None
            })
        );
        assert!(usage(&json!({"result":{"usage":{"total_tokens":48}}})).is_none());
    }

    #[test]
    fn accepts_sdk_camel_case_without_inventing_attribution() {
        let got = usage(
            &json!({"usage":{"inputTokens":2,"outputTokens":3,"cost":9.99,"model":"ignored"}}),
        )
        .unwrap();
        assert_eq!(got.model, None);
        assert_eq!(got.cost_usd, None);
        assert_eq!((got.tokens_in, got.tokens_out), (2, 3));
    }
}
