use anyhow::{bail, Result};

use crate::config::{ensure_no_spaces, find_vault_root, FilenameFormat, VaultConfig};

pub struct ConfigArgs {
    pub set: Option<String>,
    pub show: bool,
    pub archive: Option<String>,
    pub migrate: bool,
}

pub fn run(args: ConfigArgs) -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let mut config = VaultConfig::load(&vault_root)?;

    if args.show {
        let s = toml::to_string_pretty(&config)?;
        println!("{}", s);
        return Ok(());
    }

    if let Some(course) = args.archive {
        ensure_no_spaces(&course, "course name")?;
        config.active_courses.retain(|c| c != &course);
        config.save(&vault_root)?;
        println!("Archived course '{}'.", course);
        return Ok(());
    }

    if let Some(kv) = args.set {
        let (key, value) = kv
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--set requires key=value format"))?;
        apply_set(&mut config, key.trim(), value.trim())?;
        config.save(&vault_root)?;
        println!("Set {} = {}", key.trim(), value.trim());
        return Ok(());
    }

    if args.migrate {
        println!("config --migrate: not yet implemented (Phase 4).");
        return Ok(());
    }

    println!("Use --show, --set key=value, or --archive COURSE.");
    Ok(())
}

fn apply_set(config: &mut VaultConfig, key: &str, value: &str) -> Result<()> {
    match key {
        "filename_format" => {
            FilenameFormat::parse(value)?;
            config.filename_format = value.to_string();
        }
        "archive_threshold_weeks" => {
            config.archive_threshold_weeks = value
                .parse()
                .map_err(|_| anyhow::anyhow!("archive_threshold_weeks must be a number"))?;
        }
        "default_backend" => {
            config.default_backend = match value {
                "claude" => crate::config::AiBackend::Claude,
                "agy" => crate::config::AiBackend::Agy,
                "mock" => crate::config::AiBackend::Mock,
                _ => bail!("unknown backend '{}'", value),
            };
        }
        _ => bail!(crate::error::CsnotesError::InvalidConfigKey { key: key.to_string() }),
    }
    Ok(())
}
