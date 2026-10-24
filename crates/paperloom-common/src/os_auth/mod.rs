//! "Confirm it's you with your computer's login": the check behind resetting
//! the Hidden-section password when it is forgotten.
//!
//! Each system asks in its own way, and the app never sees the secret:
//! Linux and macOS show the system's own password dialog (polkit / the
//! administrator prompt), Windows asks Windows Hello (PIN, fingerprint, face)
//! or, without it, the account password. The call blocks while the dialog is
//! open, so run it off the UI thread.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

/// Why the check did not pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The dialog was closed or the answer was not accepted.
    Declined,
    /// This computer can't do the check (no dialog available, tool missing).
    Unavailable(String),
}

impl From<String> for Failure {
    /// For when the helper thread itself went wrong.
    fn from(why: String) -> Self {
        Failure::Unavailable(why)
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Declined => f.write_str("Your computer didn't confirm it. Nothing was changed."),
            Failure::Unavailable(why) => f.write_str(why),
        }
    }
}

/// Asks the system to confirm the person at the keyboard. `Ok` only when
/// the system itself said yes.
pub fn verify(reason: &str) -> Result<(), Failure> {
    #[cfg(unix)]
    {
        unix::verify(reason)
    }
    #[cfg(windows)]
    {
        windows::verify(reason)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = reason;
        Err(Failure::Unavailable("This system can't confirm your login from here.".into()))
    }
}

/// What a finished helper program said, as the check's answer. `program` is
/// named in the "not installed" message.
#[cfg_attr(windows, allow(dead_code))]
fn interpret_exit(program: &str, code: Option<i32>, stderr: &str, not_found: bool) -> Result<(), Failure> {
    if not_found {
        return Err(Failure::Unavailable(format!("{program} isn't installed, so this computer can't show its login window. Install it, or see the manual way below.")));
    }
    match code {
        Some(0) => Ok(()),
        // pkexec: 126 = not authorized or dialog dismissed, 127 = couldn't run.
        Some(127) if !stderr.to_lowercase().contains("dismissed") => Err(Failure::Unavailable(format!("{program} couldn't show a login window here ({})", stderr.trim()))),
        _ => Err(Failure::Declined),
    }
}

/// Windows Hello's answer (the name of `UserConsentVerificationResult`, or
/// `OK`/`NO` from the password fallback).
#[cfg_attr(not(windows), allow(dead_code))]
fn interpret_windows(output: &str) -> Result<(), Failure> {
    match output.trim() {
        "Verified" | "OK" => Ok(()),
        "DeviceNotPresent" | "NotConfiguredForUser" | "DisabledByPolicy" | "NOHELLO" => {
            Err(Failure::Unavailable("Windows can't ask for your login here. Set up a PIN or password for your account, or use the manual way below.".into()))
        }
        _ => Err(Failure::Declined),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_only_on_exit_zero() {
        assert_eq!(interpret_exit("pkexec", Some(0), "", false), Ok(()));
        assert_eq!(interpret_exit("pkexec", Some(126), "", false), Err(Failure::Declined));
        assert_eq!(interpret_exit("pkexec", Some(1), "", false), Err(Failure::Declined));
        assert_eq!(interpret_exit("pkexec", None, "", false), Err(Failure::Declined));
    }

    #[test]
    fn a_missing_helper_is_unavailable_not_declined() {
        assert!(matches!(interpret_exit("pkexec", None, "", true), Err(Failure::Unavailable(_))));
        assert!(matches!(interpret_exit("pkexec", Some(127), "no agent", false), Err(Failure::Unavailable(_))));
        assert_eq!(interpret_exit("pkexec", Some(127), "Dismissed", false), Err(Failure::Declined));
    }

    #[test]
    fn windows_answers() {
        assert_eq!(interpret_windows("Verified\r\n"), Ok(()));
        assert_eq!(interpret_windows("OK"), Ok(()));
        assert_eq!(interpret_windows("Canceled"), Err(Failure::Declined));
        assert_eq!(interpret_windows("NO"), Err(Failure::Declined));
        assert!(matches!(interpret_windows("NOHELLO"), Err(Failure::Unavailable(_))));
        assert_eq!(interpret_windows(""), Err(Failure::Declined));
    }
}
