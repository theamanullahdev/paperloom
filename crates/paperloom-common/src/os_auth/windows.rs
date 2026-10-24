//! Windows Hello (PIN, fingerprint, face), or the account password when
//! Hello isn't set up. Done through PowerShell so no extra system bindings
//! are linked into the app.

use super::{interpret_windows, Failure};

const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
try {
  Add-Type -AssemblyName System.Runtime.WindowsRuntime
  $verifier = [Windows.Security.Credentials.UI.UserConsentVerifier, Windows.Security.Credentials.UI, ContentType = WindowsRuntime]
  $asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
  $result = [Windows.Security.Credentials.UI.UserConsentVerificationResult, Windows.Security.Credentials.UI, ContentType = WindowsRuntime]
  $task = $asTask.MakeGenericMethod($result).Invoke($null, @($verifier::RequestVerificationAsync($env:PAPERLOOM_REASON)))
  [void]$task.Wait(-1)
  $answer = $task.Result.ToString()
} catch { $answer = 'DeviceNotPresent' }
if ($answer -eq 'DeviceNotPresent' -or $answer -eq 'NotConfiguredForUser') {
  try {
    Add-Type -AssemblyName System.DirectoryServices.AccountManagement
    $cred = Get-Credential -UserName $env:USERNAME -Message $env:PAPERLOOM_REASON
    if ($null -eq $cred) { 'NO'; exit }
    $ctx = New-Object System.DirectoryServices.AccountManagement.PrincipalContext('Machine')
    if ($ctx.ValidateCredentials($cred.UserName, $cred.GetNetworkCredential().Password)) { 'OK' } else { 'NO' }
  } catch { 'NOHELLO' }
} else { $answer }
"#;

pub fn verify(reason: &str) -> Result<(), Failure> {
    let output = crate::process::command("powershell").args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", SCRIPT]).env("PAPERLOOM_REASON", reason).output();
    match output {
        Ok(out) => interpret_windows(&String::from_utf8_lossy(&out.stdout)),
        Err(err) => Err(Failure::Unavailable(format!("Couldn't ask Windows to confirm your login ({err}). Use the manual way below."))),
    }
}
