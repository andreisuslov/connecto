//! Windows implementation of `connecto ssh` (OpenSSH Server via PowerShell)

use crate::SilentExit;
use anyhow::Result;
use colored::Colorize;
use std::process::Output;
use tokio::process::Command;

/// Check for Administrator privileges without blocking the async runtime
///
/// Delegates to the single workspace-wide elevation check in
/// `connecto_core::keys` (a cached PowerShell probe).
async fn is_elevated() -> bool {
    tokio::task::spawn_blocking(connecto_core::keys::is_windows_admin)
        .await
        .unwrap_or(false)
}

pub(super) async fn enable() -> Result<()> {
    if !is_elevated().await {
        println!(
            "{} This command requires Administrator privileges.",
            "✗".red()
        );
        println!();
        println!("Please run PowerShell as Administrator and try again:");
        println!("  {}", "connecto ssh on".cyan());
        return Err(SilentExit.into());
    }

    println!("{} Enabling OpenSSH Server...", "→".cyan());
    println!();

    if sshd_exists().await? {
        println!("{} OpenSSH Server already installed.", "✓".green());
    } else {
        install().await?;
    }

    // Start the sshd service
    println!("{} Starting SSH service...", "→".cyan());

    let start_output = Command::new("powershell")
        .args(["-Command", "Start-Service sshd"])
        .output()
        .await?;

    if !start_output.status.success() {
        let stderr = String::from_utf8_lossy(&start_output.stderr);
        if !stderr.contains("already") {
            println!("{} Failed to start SSH service.", "✗".red());
            if !stderr.is_empty() {
                println!("{}", stderr.dimmed());
            }
            return Err(SilentExit.into());
        }
    }

    println!("{} SSH service started.", "✓".green());

    // Set to automatic startup
    println!("{} Configuring automatic startup...", "→".cyan());

    let auto_output = Command::new("powershell")
        .args([
            "-Command",
            "Set-Service -Name sshd -StartupType 'Automatic'",
        ])
        .output()
        .await?;

    if !auto_output.status.success() {
        println!("{} Warning: Could not set automatic startup.", "⚠".yellow());
    } else {
        println!("{} SSH will start automatically on boot.", "✓".green());
    }

    // Configure firewall rule
    println!("{} Configuring firewall...", "→".cyan());

    let firewall_output = Command::new("powershell")
        .args([
            "-Command",
            r#"
            $rule = Get-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -ErrorAction SilentlyContinue
            if (-not $rule) {
                New-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -DisplayName 'OpenSSH Server (sshd)' -Enabled True -Direction Inbound -Protocol TCP -Action Allow -LocalPort 22
                'created'
            } else {
                Enable-NetFirewallRule -Name 'OpenSSH-Server-In-TCP'
                'enabled'
            }
            "#,
        ])
        .output()
        .await?;

    if firewall_output.status.success() {
        println!("{} Firewall configured for SSH (port 22).", "✓".green());
    } else {
        println!("{} Warning: Could not configure firewall.", "⚠".yellow());
    }

    super::print_success_message();
    Ok(())
}

/// Windows capability name of the inbox OpenSSH Server
const CAPABILITY: &str = "OpenSSH.Server~~~~0.0.1.0";

/// Downloads the latest Win32-OpenSSH x64 MSI from GitHub and installs only
/// its Server feature. Failures are thrown, so they land on stderr.
const MSI_INSTALL_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$release = Invoke-RestMethod 'https://api.github.com/repos/PowerShell/Win32-OpenSSH/releases/latest'
$asset = $release.assets | Where-Object { $_.name -like 'OpenSSH-Win64-*.msi' } | Select-Object -First 1
if (-not $asset) { throw 'The latest Win32-OpenSSH release has no OpenSSH-Win64 MSI' }
$msi = Join-Path $env:TEMP $asset.name
Invoke-WebRequest $asset.browser_download_url -OutFile $msi -UseBasicParsing
$p = Start-Process msiexec.exe -ArgumentList '/i', "`"$msi`"", '/qn', 'ADDLOCAL=Server' -Wait -PassThru
Remove-Item $msi -ErrorAction SilentlyContinue
if ($p.ExitCode -notin 0, 3010) { throw "msiexec exited with code $($p.ExitCode)" }
"#;

async fn powershell(script: &str) -> Result<Output> {
    Ok(Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .await?)
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn print_stderr(output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        println!("{}", stderr.trim_end().dimmed());
    }
}

async fn sshd_exists() -> Result<bool> {
    let output = powershell(
        "Get-Service sshd -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name",
    )
    .await?;
    Ok(!stdout_of(&output).is_empty())
}

async fn add_capability() -> Result<Output> {
    powershell(&format!(
        "Add-WindowsCapability -Online -Name {CAPABILITY} | Out-Null"
    ))
    .await
}

/// Install OpenSSH Server until the sshd service exists.
///
/// Success is judged by the service, never by an installer's exit code: on a
/// Windows image with a torn servicing store, Add-WindowsCapability reports
/// success while the package stays staged ("InstallPending") and no service
/// is registered. Removing the capability clears that record; if Windows
/// cannot provide the capability at all, the Win32-OpenSSH MSI is used.
async fn install() -> Result<()> {
    println!("{} Installing OpenSSH Server...", "→".cyan());

    let has_capabilities = !stdout_of(
        &powershell("Get-Command Add-WindowsCapability -ErrorAction SilentlyContinue").await?,
    )
    .is_empty();

    if has_capabilities {
        let added = add_capability().await?;
        if sshd_exists().await? {
            println!("{} OpenSSH Server installed.", "✓".green());
            return Ok(());
        }

        let state = stdout_of(
            &powershell(&format!(
                "(Get-WindowsCapability -Online -Name {CAPABILITY}).State"
            ))
            .await?,
        );
        println!(
            "{} Windows reports OpenSSH Server as '{}', but the sshd service does not exist.",
            "!".yellow(),
            state
        );
        print_stderr(&added);

        if state == "InstallPending" || state == "Installed" {
            println!(
                "{} Removing the broken OpenSSH Server install...",
                "→".cyan()
            );
            let removed = powershell(&format!(
                "(Remove-WindowsCapability -Online -Name {CAPABILITY}).RestartNeeded"
            ))
            .await?;
            if stdout_of(&removed) == "True" {
                println!(
                    "{} Windows must restart to finish removing the broken install.",
                    "✗".red()
                );
                println!("  Restart this machine, then run this command again.");
                return Err(SilentExit.into());
            }
            print_stderr(&removed);

            let added = add_capability().await?;
            if sshd_exists().await? {
                println!("{} OpenSSH Server installed.", "✓".green());
                return Ok(());
            }
            print_stderr(&added);
        }
    }

    println!(
        "{} Installing OpenSSH Server from the Win32-OpenSSH release (MSI)...",
        "→".cyan()
    );
    let msi = powershell(MSI_INSTALL_SCRIPT).await?;
    if sshd_exists().await? {
        println!("{} OpenSSH Server installed.", "✓".green());
        return Ok(());
    }

    println!("{} Failed to install OpenSSH Server.", "✗".red());
    print_stderr(&msi);
    println!();
    println!(
        "Install it manually from {}, then run {} again.",
        "https://github.com/PowerShell/Win32-OpenSSH/releases".cyan(),
        "connecto ssh on".cyan()
    );
    Err(SilentExit.into())
}

pub(super) async fn disable() -> Result<()> {
    if !is_elevated().await {
        println!(
            "{} This command requires Administrator privileges.",
            "✗".red()
        );
        println!();
        println!("Please run PowerShell as Administrator and try again:");
        println!("  {}", "connecto ssh off".cyan());
        return Err(SilentExit.into());
    }

    println!("{} Disabling OpenSSH Server...", "→".cyan());

    // Stop the service
    let stop_output = Command::new("powershell")
        .args([
            "-Command",
            "Stop-Service sshd -ErrorAction SilentlyContinue",
        ])
        .output()
        .await?;

    let stop_ok = stop_output.status.success();
    if stop_ok {
        println!("{} SSH service stopped.", "✓".green());
    }

    // Disable automatic startup
    let disable_output = Command::new("powershell")
        .args([
            "-Command",
            "Set-Service -Name sshd -StartupType 'Disabled' -ErrorAction SilentlyContinue",
        ])
        .output()
        .await?;

    let disable_ok = disable_output.status.success();
    if disable_ok {
        println!("{} SSH automatic startup disabled.", "✓".green());
    }

    // Only claim success when both steps actually succeeded.
    if !(stop_ok && disable_ok) {
        println!();
        println!("{} Failed to disable SSH server.", "✗".red());
        return Err(SilentExit.into());
    }

    println!();
    println!("{}", "SSH Server is now disabled.".yellow());
    println!();

    Ok(())
}

pub(super) async fn status() -> Result<()> {
    let status_output = Command::new("powershell")
        .args([
            "-Command",
            "Get-Service sshd -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Status",
        ])
        .output()
        .await?;

    let status = String::from_utf8_lossy(&status_output.stdout)
        .trim()
        .to_string();

    if status.is_empty() {
        println!(
            "{} OpenSSH Server is {}",
            "•".red(),
            "not installed".red().bold()
        );
        println!();
        println!("Install and enable with: {}", "connecto ssh on".cyan());
        return Ok(());
    }

    match status.as_str() {
        "Running" => {
            println!("{} SSH server is {}", "•".green(), "running".green().bold());
        }
        "Stopped" => {
            println!(
                "{} SSH server is {}",
                "•".yellow(),
                "stopped".yellow().bold()
            );
        }
        _ => {
            println!("{} SSH server status: {}", "•".dimmed(), status);
        }
    }

    // Check startup type
    let startup_output = Command::new("powershell")
        .args([
            "-Command",
            "Get-Service sshd | Select-Object -ExpandProperty StartType",
        ])
        .output()
        .await?;

    let startup = String::from_utf8_lossy(&startup_output.stdout)
        .trim()
        .to_string();

    match startup.as_str() {
        "Automatic" => {
            println!("{} Starts automatically on boot", "•".green());
        }
        "Disabled" => {
            println!(
                "{} Automatic startup is {}",
                "•".yellow(),
                "disabled".yellow()
            );
        }
        _ => {
            println!("{} Startup type: {}", "•".dimmed(), startup);
        }
    }

    // Check firewall
    let firewall_output = Command::new("powershell")
        .args(["-Command", "Get-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Enabled"])
        .output()
        .await?;

    let firewall = String::from_utf8_lossy(&firewall_output.stdout)
        .trim()
        .to_string();

    if firewall == "True" {
        println!("{} Firewall allows SSH (port 22)", "•".green());
    } else {
        println!("{} Firewall rule not configured", "•".yellow());
    }

    println!();
    Ok(())
}
