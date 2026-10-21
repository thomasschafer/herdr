//! Restoring allowlisted foreground programs, such as editors, after a Herdr
//! restart. Herdr remembers the command line of an allowlisted program while it
//! runs in a pane and types it into the restored pane's shell, reusing the
//! deferred agent resume launch.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

pub(crate) const RESTORE_COMMAND_SOURCE: &str = "herdr:restore_command";

const MAX_RESTORE_KEY_LEN: usize = 64;

/// A program name from `[session] restore_commands`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct RestoreCommandName(String);

impl RestoreCommandName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RestoreCommandName {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if crate::agent_resume::is_plain_command_name(&value) {
            Ok(Self(value))
        } else {
            Err(format!(
                "restore_commands entries must be plain command names, not paths or options: {value:?}"
            ))
        }
    }
}

/// Opaque per-pane identity that survives Herdr restarts, exported to pane
/// processes so programs can find state they saved for the same pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreKey(String);

static NEXT_RESTORE_KEY: AtomicU64 = AtomicU64::new(1);

impl RestoreKey {
    pub fn generate() -> Self {
        let micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_micros())
            .unwrap_or(0);
        let counter = NEXT_RESTORE_KEY.fetch_add(1, Ordering::Relaxed);
        Self(format!(
            "rk_{micros:x}_{:x}_{counter:x}",
            std::process::id()
        ))
    }

    /// Keys end up in environment variables and in file names chosen by other
    /// programs, so saved values must stay within a conservative charset.
    pub fn parse(value: &str) -> Option<Self> {
        let valid = !value.is_empty()
            && value.len() <= MAX_RESTORE_KEY_LEN
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
        valid.then(|| Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The leader of a pane's foreground job when it is neither the pane shell nor
/// an agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundCommand {
    pub process_name: String,
    pub argv: Option<Vec<String>>,
}

/// The leader of a pane's foreground job as last seen, kept so the allowlist
/// can be applied again when it changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ForegroundObservation {
    /// No job could be inspected yet, or since the pane's process changed.
    #[default]
    Unobserved,
    /// `None` when an agent leads the job or the job has no leader.
    Observed(Option<ForegroundCommand>),
}

/// The command to type into the restored shell, or `None` when the program is
/// not allowlisted or its command line cannot be replayed safely.
///
/// The first argument is replaced by the allowlisted name so a program started
/// through a path is restored through `PATH`, which is all restore can type.
pub fn restorable_argv(
    command: &ForegroundCommand,
    allowlist: &[RestoreCommandName],
) -> Option<Vec<String>> {
    let argv0_name = command
        .argv
        .as_ref()
        .and_then(|argv| argv.first())
        .and_then(|argv0| Path::new(argv0).file_name())
        .and_then(|name| name.to_str());
    let name = allowlist
        .iter()
        .find(|name| name.as_str() == command.process_name || Some(name.as_str()) == argv0_name)?;
    let Some(argv) = command.argv.as_ref().filter(|argv| !argv.is_empty()) else {
        tracing::warn!(
            command = name.as_str(),
            "cannot remember allowlisted restore command without its arguments"
        );
        return None;
    };
    let mut restored = argv.clone();
    restored[0] = name.as_str().to_string();
    if let Err(err) = crate::agent_resume::validate_resume_argv(&restored) {
        tracing::warn!(
            command = name.as_str(),
            err = %err,
            "cannot remember allowlisted restore command"
        );
        return None;
    }
    Some(restored)
}

/// Builds the launch plan for a saved command when its program is still
/// allowlisted.
pub fn plan(
    argv: &[String],
    allowlist: &[RestoreCommandName],
    cwd: &Path,
    restore_key: Option<&RestoreKey>,
) -> Option<crate::agent_resume::AgentResumePlan> {
    let command = argv.first()?;
    if !allowlist.iter().any(|name| name.as_str() == command) {
        return None;
    }
    crate::agent_resume::validate_resume_argv(argv).ok()?;
    // Two panes may run the same command in the same directory, so the pane's
    // own key, when known, decides whether a plan is a duplicate.
    let identity = restore_key.map_or_else(
        || format!("{}\u{0}{}", cwd.display(), argv.join("\u{0}")),
        |key| key.as_str().to_string(),
    );
    Some(crate::agent_resume::AgentResumePlan {
        agent: command.clone(),
        argv: argv.to_vec(),
        dedupe_key: format!("{RESTORE_COMMAND_SOURCE}\u{0}{identity}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(values: &[&str]) -> Vec<RestoreCommandName> {
        values
            .iter()
            .map(|value| RestoreCommandName::try_from(value.to_string()).unwrap())
            .collect()
    }

    fn command(process_name: &str, argv: Option<&[&str]>) -> ForegroundCommand {
        ForegroundCommand {
            process_name: process_name.into(),
            argv: argv.map(|argv| argv.iter().map(|arg| arg.to_string()).collect()),
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn restore_command_names_must_be_plain_command_names() {
        assert!(RestoreCommandName::try_from("hx".to_string()).is_ok());
        assert!(RestoreCommandName::try_from("nvim.app".to_string()).is_ok());
        for invalid in ["", "/usr/bin/hx", "-hx", "h x", "hx;rm"] {
            assert!(
                RestoreCommandName::try_from(invalid.to_string()).is_err(),
                "{invalid:?} must be rejected"
            );
        }
    }

    #[test]
    fn generated_restore_keys_are_unique_and_parse_back() {
        let first = RestoreKey::generate();
        let second = RestoreKey::generate();
        assert_ne!(first, second);
        assert_eq!(RestoreKey::parse(first.as_str()), Some(first));
    }

    #[test]
    fn restore_key_parse_rejects_values_unsafe_for_env_or_file_names() {
        for invalid in ["", "../escape", "a/b", "a b", "a\nb", &"k".repeat(65)] {
            assert_eq!(RestoreKey::parse(invalid), None, "{invalid:?}");
        }
        assert!(RestoreKey::parse("rk_19a2b_3c_1").is_some());
    }

    #[test]
    fn restorable_argv_matches_process_name_or_argv0_and_restores_through_path() {
        let allowlist = names(&["hx"]);
        assert_eq!(
            restorable_argv(
                &command("hx", Some(&["/Users/me/.cargo/bin/hx", "src/main.rs"])),
                &allowlist
            ),
            Some(strings(&["hx", "src/main.rs"]))
        );
        assert_eq!(
            restorable_argv(&command("helix-term", Some(&["hx"])), &allowlist),
            Some(strings(&["hx"]))
        );
    }

    #[test]
    fn restorable_argv_ignores_programs_outside_the_allowlist() {
        assert_eq!(
            restorable_argv(&command("vim", Some(&["vim", "a"])), &names(&["hx"])),
            None
        );
        assert_eq!(
            restorable_argv(&command("hx", Some(&["hx"])), &names(&[])),
            None
        );
    }

    #[test]
    fn restorable_argv_rejects_unknown_or_unsafe_arguments() {
        let allowlist = names(&["hx"]);
        assert_eq!(restorable_argv(&command("hx", None), &allowlist), None);
        assert_eq!(restorable_argv(&command("hx", Some(&[])), &allowlist), None);
        assert_eq!(
            restorable_argv(&command("hx", Some(&["hx", "it's.txt"])), &allowlist),
            None
        );
    }

    #[test]
    fn plan_requires_the_program_to_still_be_allowlisted() {
        let argv = strings(&["hx", "a.rs"]);
        let cwd = Path::new("/repo");
        assert!(plan(&argv, &names(&["hx"]), cwd, None).is_some());
        assert!(plan(&argv, &names(&["nvim"]), cwd, None).is_none());
        assert!(plan(&strings(&["/bin/hx"]), &names(&["hx"]), cwd, None).is_none());
    }

    #[test]
    fn plan_dedupes_by_restore_key_when_known() {
        let argv = strings(&["hx"]);
        let allowlist = names(&["hx"]);
        let cwd = Path::new("/repo");
        let first = RestoreKey::parse("rk_a").unwrap();
        let second = RestoreKey::parse("rk_b").unwrap();
        let plan_for = |key| plan(&argv, &allowlist, cwd, key).unwrap().dedupe_key;

        assert_ne!(plan_for(Some(&first)), plan_for(Some(&second)));
        assert_eq!(plan_for(Some(&first)), plan_for(Some(&first)));
        assert_eq!(plan_for(None), plan_for(None));
        assert_ne!(
            plan_for(None),
            plan(&argv, &allowlist, Path::new("/other"), None)
                .unwrap()
                .dedupe_key
        );
    }

    #[test]
    fn plan_is_not_mistaken_for_an_agent() {
        let plan = plan(&strings(&["hx"]), &names(&["hx"]), Path::new("/repo"), None).unwrap();
        assert_eq!(crate::detect::parse_agent_label(&plan.agent), None);
    }
}
