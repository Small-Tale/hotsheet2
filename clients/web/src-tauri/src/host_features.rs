//! Opt-in host features the desktop shell forwards to its bundled local host (HS2-ZF70Y9).
//!
//! The local host (`clients/web/src/host-features.ts`) owns the feature names and rejects an
//! unknown one at startup. The shell only carries `--feature <name>`, `--feature=<name>` and
//! `--feature:<name>` from its own command line to the host's, so
//! `"Hot Sheet.app/Contents/MacOS/hotsheet-desktop" --feature ux-demo` serves `/ux-demo`. The
//! host also inherits the shell's environment, so `HOTSHEET_FEATURES=ux-demo` works as well.

const FEATURE_FLAG: &str = "--feature";

/// The host arguments for every feature named on the shell's command line (without argv[0]).
/// Other arguments are left to the shell. A trailing `--feature` with no name is passed through
/// as-is so the host reports it.
pub fn forwarded_feature_arguments<I, S>(arguments: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut forwarded = Vec::new();
    let mut arguments = arguments.into_iter().map(Into::into);
    while let Some(argument) = arguments.next() {
        if argument == FEATURE_FLAG {
            forwarded.push(argument);
            if let Some(name) = arguments.next() {
                forwarded.push(name);
            }
        } else if let Some(name) = argument
            .strip_prefix("--feature=")
            .or_else(|| argument.strip_prefix("--feature:"))
        {
            forwarded.push(FEATURE_FLAG.to_owned());
            forwarded.push(name.to_owned());
        }
    }
    forwarded
}

#[cfg(test)]
mod tests {
    use super::forwarded_feature_arguments;

    #[test]
    fn forwards_every_feature_spelling_and_nothing_else() {
        assert!(forwarded_feature_arguments(Vec::<String>::new()).is_empty());
        assert!(forwarded_feature_arguments(["-psn_0_123", "--verbose"]).is_empty());
        assert_eq!(
            forwarded_feature_arguments([
                "--feature",
                "ux-demo",
                "-psn_0_1",
                "--feature=ux-demo",
                "--feature:ux-demo,other"
            ]),
            [
                "--feature",
                "ux-demo",
                "--feature",
                "ux-demo",
                "--feature",
                "ux-demo,other"
            ]
        );
    }

    #[test]
    fn passes_a_nameless_flag_through_for_the_host_to_reject() {
        assert_eq!(forwarded_feature_arguments(["--feature"]), ["--feature"]);
    }
}
