use anyhow::{Result, bail};
use clap::CommandFactory;
use clap_complete::{Shell, generate};
use std::io::{self, Write};
use std::path::PathBuf;

/// Execute the completions command.
///
/// - No args: output completions for auto-detected shell (for `eval "$(sweeprs completions)"`)
/// - `sweeprs completions <shell>`: output completions for a specific shell
/// - `sweeprs completions --install`: install hook into shell config
pub fn execute(shell: Option<Shell>, install: bool) -> Result<()> {
    let shell = match shell {
        Some(s) => s,
        None => detect_shell()?,
    };

    if install {
        install_completions(shell)?;
    } else {
        generate_completions(shell)?;
    }

    Ok(())
}

/// Detect the current shell from environment variables.
fn detect_shell() -> Result<Shell> {
    if let Ok(shell_path) = std::env::var("SHELL") {
        let shell_name = shell_path.rsplit('/').next().unwrap_or("");
        match shell_name {
            "bash" => return Ok(Shell::Bash),
            "zsh" => return Ok(Shell::Zsh),
            "fish" => return Ok(Shell::Fish),
            _ => {}
        }
    }

    if std::env::var("ZSH_VERSION").is_ok() {
        return Ok(Shell::Zsh);
    }
    if std::env::var("BASH_VERSION").is_ok() {
        return Ok(Shell::Bash);
    }
    if std::env::var("FISH_VERSION").is_ok() {
        return Ok(Shell::Fish);
    }

    bail!(
        "could not detect shell automatically -- please specify:\n  \
         sweeprs completions bash\n  \
         sweeprs completions zsh\n  \
         sweeprs completions fish"
    );
}

/// Generate completions and write to stdout.
fn generate_completions(shell: Shell) -> Result<()> {
    let mut cmd = crate::Cli::command();
    let name = cmd.get_name().to_string();

    let mut buf = Vec::new();
    generate(shell, &mut cmd, name, &mut buf);
    io::stdout().write_all(&buf)?;

    Ok(())
}

/// Install a dynamic completion hook into the user's shell config.
fn install_completions(shell: Shell) -> Result<()> {
    let (config_file, hook_content) = match shell {
        Shell::Bash => (find_bash_config()?, generate_bash_hook()),
        Shell::Zsh => (find_zsh_config()?, generate_zsh_hook()),
        Shell::Fish => (find_fish_config()?, generate_fish_hook()),
        _ => bail!("automatic installation is not supported for {shell:?} -- generate manually with: sweeprs completions {shell:?}"),
    };

    eprintln!("Install shell completions");
    eprintln!();
    eprintln!("  Shell: {shell:?}");
    eprintln!("  Config: {}", config_file.display());
    eprintln!();

    // Check if hook already exists
    if config_file.exists() {
        let content = std::fs::read_to_string(&config_file)?;
        if content.contains("sweeprs completions") {
            // Check if zsh hook needs compdef update
            let needs_update =
                matches!(shell, Shell::Zsh) && !content.contains("compdef _sweeprs");

            if needs_update {
                eprintln!("  Existing hook is outdated, replacing...");
                let updated = remove_completion_block(&content);
                std::fs::write(&config_file, updated)?;
            } else {
                eprintln!("  Completions already installed and up-to-date.");
                return Ok(());
            }
        }
    }

    // Create parent directory if needed
    if let Some(parent) = config_file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Append hook
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&config_file)?;

    writeln!(file)?;
    writeln!(file, "# sweeprs shell completions")?;
    writeln!(
        file,
        "# Added by: sweeprs completions {shell:?} --install"
    )?;
    writeln!(
        file,
        "# Dynamically loads from the binary so it auto-updates on upgrade"
    )?;
    write!(file, "{hook_content}")?;

    eprintln!("  Completions installed successfully.");
    eprintln!();
    eprintln!("  Reload your shell or run:");
    eprintln!("    source {}", config_file.display());

    Ok(())
}

/// Remove an existing sweeprs completion block from shell config content.
fn remove_completion_block(content: &str) -> String {
    let mut result = Vec::new();
    let mut in_block = false;

    for line in content.lines() {
        if line.contains("# sweeprs") && line.contains("completion") {
            in_block = true;
            continue;
        }
        if line.contains("Added by: sweeprs completions") {
            in_block = true;
            continue;
        }

        if in_block {
            let trimmed = line.trim();
            // Block end markers
            if trimmed == "fi" || trimmed == "end" {
                continue;
            }
            // Lines that are part of the block
            if trimmed.contains("sweeprs")
                || trimmed.contains("compdef")
                || trimmed.starts_with("if ")
                || trimmed.starts_with("eval")
                || trimmed.starts_with('#')
                || trimmed.is_empty()
            {
                continue;
            }
            // Reached content outside the block
            in_block = false;
            result.push(line.to_string());
            continue;
        }

        result.push(line.to_string());
    }

    result.join("\n")
}

// -- Shell-specific hooks -----------------------------------------------------

fn generate_bash_hook() -> String {
    r#"if command -v sweeprs &> /dev/null; then
  eval "$(sweeprs completions bash 2>/dev/null || true)"
fi
"#
    .to_string()
}

fn generate_zsh_hook() -> String {
    r#"if command -v sweeprs &> /dev/null; then
  eval "$(sweeprs completions zsh 2>/dev/null || true)"
  compdef _sweeprs sweeprs 2>/dev/null
fi
"#
    .to_string()
}

fn generate_fish_hook() -> String {
    r"if command -v sweeprs &> /dev/null
  sweeprs completions fish 2>/dev/null | source
end
"
    .to_string()
}

// -- Shell config file detection -----------------------------------------------

fn find_bash_config() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("could not find home directory"))?;

    for name in &[".bashrc", ".bash_profile", ".profile"] {
        let candidate = home.join(name);
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Ok(home.join(".bashrc"))
}

fn find_zsh_config() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("could not find home directory"))?;

    for name in &[".zshrc", ".zprofile"] {
        let candidate = home.join(name);
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Ok(home.join(".zshrc"))
}

fn find_fish_config() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("could not find config directory"))?;

    Ok(config_dir.join("fish").join("config.fish"))
}
