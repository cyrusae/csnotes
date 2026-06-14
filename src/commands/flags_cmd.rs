use anyhow::{bail, Result};
use chrono::Utc;

use crate::config::{find_vault_root, VaultConfig};
use crate::flags::FlagStore;

pub enum FlagsSubcommand {
    List {
        all: bool,
    },
    Resolve {
        id: String,
        follow_up: Option<String>,
    },
    Show {
        id: String,
    },
}

pub fn run(sub: FlagsSubcommand) -> Result<()> {
    let vault_root = find_vault_root(&std::env::current_dir()?)?;
    let config = VaultConfig::load(&vault_root)?;
    let flags_path = vault_root.join(&config.generated_dir).join("flags.json");
    let mut store = FlagStore::load(&flags_path).unwrap_or_default();

    match sub {
        FlagsSubcommand::List { all } => {
            let actionable: Vec<_> = store.open_actionable().collect();
            let threads: Vec<_> = store.open_threads().collect();

            if actionable.is_empty() && (threads.is_empty() || !all) {
                println!("No open actionable flags.");
            }

            for flag in &actionable {
                println!("[{}] {} — {}", flag.id, flag.display_kind(), flag.message);
                if let Some(ref path) = flag.path {
                    println!("     {}", path);
                }
            }

            if all {
                for flag in &threads {
                    println!("[{}] {} — {}", flag.id, flag.display_kind(), flag.message);
                }
                // Changelog entries (resolved ambiguity flags)
                let changelog: Vec<_> = store
                    .flags
                    .iter()
                    .filter(|f| f.kind == crate::report::FlagKind::Ambiguity)
                    .collect();
                if !changelog.is_empty() {
                    println!("\nChangelog:");
                    for flag in changelog {
                        println!("  [{}] {}", flag.id, flag.message);
                    }
                }
            }
        }

        FlagsSubcommand::Resolve { id, follow_up } => {
            if !store.resolve(&id, Utc::now(), follow_up) {
                bail!("Flag '{}' not found.", id);
            }
            store.save(&flags_path)?;
            println!("Flag '{}' resolved.", id);
        }

        FlagsSubcommand::Show { id } => {
            let flag = store.flags.iter().find(|f| f.id == id);
            match flag {
                Some(f) => {
                    println!("ID:      {}", f.id);
                    println!("Kind:    {}", f.display_kind());
                    println!("Open:    {}", f.open);
                    println!("Message: {}", f.message);
                    if let Some(ref p) = f.path {
                        println!("Path:    {}", p);
                    }
                    if let Some(ref a) = f.anchor {
                        println!("Anchor:  {}", a);
                    }
                    println!("Created: {}", f.created_at.format("%Y-%m-%d %H:%M UTC"));
                    if let Some(ref r) = f.resolved_at {
                        println!("Resolved: {}", r.format("%Y-%m-%d %H:%M UTC"));
                    }
                    if let Some(ref fu) = f.follow_up {
                        println!("Follow-up: {}", fu);
                    }
                }
                None => bail!("Flag '{}' not found.", id),
            }
        }
    }

    Ok(())
}
