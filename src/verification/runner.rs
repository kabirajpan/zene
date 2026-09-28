use std::process::Command;

/// The result of running a verification command.
#[derive(Debug, Clone)]
pub enum VerificationResult {
    /// Command exited with code 0.
    Pass,
    /// Command exited non-zero or failed to spawn.
    Fail {
        exit_code: i32,
        stdout: String,
        stderr: String,
    },
}

impl VerificationResult {
    pub fn is_pass(&self) -> bool {
        matches!(self, Self::Pass)
    }

    /// Format the failure as a feedback string to re-inject into the agentic loop.
    pub fn as_feedback(&self, command: &str) -> String {
        match self {
            Self::Pass => format!("Verification passed: `{}`", command),
            Self::Fail { exit_code, stdout, stderr } => {
                let mut msg = format!(
                    "Verification failed: `{}` exited with code {}.\n",
                    command, exit_code
                );
                if !stdout.trim().is_empty() {
                    msg.push_str(&format!("STDOUT:\n{}\n", stdout.trim_end()));
                }
                if !stderr.trim().is_empty() {
                    msg.push_str(&format!("STDERR:\n{}\n", stderr.trim_end()));
                }
                msg.push_str("Please fix the errors above and retry.");
                msg
            }
        }
    }
}

/// Runs a shell command and returns whether it passed or failed.
/// The command is fully dynamic — decided by the caller (agent/model), not hardcoded.
pub struct VerificationRunner;

impl VerificationRunner {
    /// Execute `command` in an optional `cwd` directory.
    /// Returns `Pass` on exit code 0, `Fail` otherwise.
    pub fn run(command: &str, cwd: Option<&str>) -> VerificationResult {
        let mut cmd = if cfg!(target_os = "windows") {
            let mut c = Command::new("cmd");
            c.args(["/C", command]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", command]);
            c
        };

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        match cmd.output() {
            Ok(output) => {
                let code = output.status.code().unwrap_or(-1);
                if output.status.success() {
                    VerificationResult::Pass
                } else {
                    VerificationResult::Fail {
                        exit_code: code,
                        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                    }
                }
            }
            Err(e) => VerificationResult::Fail {
                exit_code: -1,
                stdout: String::new(),
                stderr: format!("Failed to spawn command '{}': {}", command, e),
            },
        }
    }
}
