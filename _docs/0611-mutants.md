Found 784 mutants to test
ok       Unmutated baseline in 16s build + 2s test
MISSED   src/audit.rs:34:9: replace AuditResult::print with () in 0s build + 1s test
MISSED   src/audit.rs:34:44: replace && with || in AuditResult::print in 1s build + 1s test
MISSED   src/audit.rs:158:24: replace > with < in invariant_suite in 0s build + 1s test
MISSED   src/audit.rs:206:66: replace == with != in invariant_suite in 1s build + 1s test
MISSED   src/audit.rs:273:24: replace > with == in audit_vault in 0s build + 1s test
MISSED   src/audit.rs:273:24: replace > with < in audit_vault in 0s build + 1s test
MISSED   src/audit.rs:273:24: replace > with >= in audit_vault in 0s build + 1s test
MISSED   src/audit.rs:286:62: replace == with != in audit_vault in 1s build + 1s test
MISSED   src/audit.rs:300:28: replace == with != in audit_vault in 1s build + 1s test
MISSED   src/audit.rs:302:28: delete ! in audit_vault in 0s build + 1s test
MISSED   src/audit.rs:348:66: replace == with != in reindex in 0s build + 1s test
MISSED   src/audit.rs:383:24: delete ! in reindex in 1s build + 1s test
MISSED   src/audit.rs:391:20: delete ! in reindex in 0s build + 1s test
MISSED   src/audit.rs:392:48: replace && with || in reindex in 1s build + 1s test
MISSED   src/audit.rs:392:30: replace == with != in reindex in 1s build + 1s test
MISSED   src/audit.rs:392:58: replace == with != in reindex in 0s build + 3s test
MISSED   src/audit.rs:399:32: replace > with == in reindex in 1s build + 1s test
MISSED   src/audit.rs:399:32: replace > with < in reindex in 1s build + 1s test
MISSED   src/audit.rs:399:32: replace > with >= in reindex in 1s build + 1s test
MISSED   src/audit.rs:416:25: replace && with || in reindex in 1s build + 1s test
MISSED   src/audit.rs:415:34: replace > with == in reindex in 1s build + 1s test
MISSED   src/audit.rs:415:34: replace > with < in reindex in 1s build + 1s test
MISSED   src/audit.rs:415:34: replace > with >= in reindex in 0s build + 1s test
MISSED   src/audit.rs:436:5: replace check_block_id_anchor -> Result<()> with Ok(()) in 1s build + 1s test
MISSED   src/audit.rs:455:5: replace check_embed_line_present -> Result<()> with Ok(()) in 1s build + 1s test
MISSED   src/audit.rs:456:8: delete ! in check_embed_line_present in 1s build + 1s test
MISSED   src/audit.rs:462:31: replace && with || in check_embed_line_present in 1s build + 1s test
MISSED   src/audit.rs:580:5: replace collect_fixes -> Result<Vec<FixItem>> with Ok(vec![]) in 1s build + 1s test
MISSED   src/audit.rs:583:8: delete ! in collect_fixes in 1s build + 1s test
MISSED   src/audit.rs:590:62: replace == with != in collect_fixes in 1s build + 1s test
MISSED   src/audit.rs:601:20: replace == with != in collect_fixes in 1s build + 1s test
MISSED   src/audit.rs:603:20: delete ! in collect_fixes in 1s build + 1s test
MISSED   src/audit.rs:629:5: replace apply_fixes -> Result<usize> with Ok(0) in 1s build + 2s test
MISSED   src/audit.rs:629:5: replace apply_fixes -> Result<usize> with Ok(1) in 2s build + 2s test
MISSED   src/audit.rs:643:25: replace += with -= in apply_fixes in 2s build + 1s test
MISSED   src/audit.rs:643:25: replace += with *= in apply_fixes in 1s build + 2s test
MISSED   src/audit.rs:660:62: replace == with != in check_orphan_atomics in 1s build + 1s test
MISSED   src/backend.rs:63:5: replace run_interactive -> std::io::Result<std::process::ExitStatus> with Ok(Default::default()) in 1s build + 1s test
MISSED   src/backend.rs:111:9: replace <impl BackendLauncher for ClaudeBackend>::launch -> Result<()> with Ok(()) in 1s build + 1s test
MISSED   src/backend.rs:120:12: delete ! in <impl BackendLauncher for ClaudeBackend>::launch in 1s build + 1s test
MISSED   src/backend.rs:130:9: replace <impl BackendLauncher for ClaudeBackend>::backend_name -> &'static str with "" in 1s build + 1s test
MISSED   src/backend.rs:130:9: replace <impl BackendLauncher for ClaudeBackend>::backend_name -> &'static str with "xyzzy" in 1s build + 1s test
MISSED   src/backend.rs:150:9: replace <impl BackendLauncher for AgyBackend>::launch -> Result<()> with Ok(()) in 1s build + 1s test
MISSED   src/backend.rs:175:12: delete ! in <impl BackendLauncher for AgyBackend>::launch in 1s build + 1s test
MISSED   src/backend.rs:185:9: replace <impl BackendLauncher for AgyBackend>::backend_name -> &'static str with "" in 1s build + 1s test
MISSED   src/backend.rs:185:9: replace <impl BackendLauncher for AgyBackend>::backend_name -> &'static str with "xyzzy" in 1s build + 1s test
MISSED   src/backend.rs:246:9: replace <impl BackendLauncher for MockBackend>::backend_name -> &'static str with "" in 3s build + 1s test
MISSED   src/backend.rs:246:9: replace <impl BackendLauncher for MockBackend>::backend_name -> &'static str with "xyzzy" in 2s build + 1s test
MISSED   src/config.rs:24:9: replace <impl std::fmt::Display for AiBackend>::fmt -> std::fmt::Result with Ok(Default::default()) in 3s build + 1s test
MISSED   src/config.rs:120:40: replace default_artifacts_dir -> String with String::new() in 1s build + 2s test
MISSED   src/config.rs:120:40: replace default_artifacts_dir -> String with "xyzzy".into() in 1s build + 1s test
MISSED   src/config.rs:121:38: replace default_sources_dir -> String with String::new() in 1s build + 1s test
MISSED   src/config.rs:121:38: replace default_sources_dir -> String with "xyzzy".into() in 1s build + 1s test
MISSED   src/config.rs:123:40: replace default_generated_dir -> String with String::new() in 1s build + 1s test
MISSED   src/config.rs:123:40: replace default_generated_dir -> String with "xyzzy".into() in 1s build + 1s test
MISSED   src/config.rs:124:38: replace default_csnotes_dir -> String with String::new() in 1s build + 1s test
MISSED   src/config.rs:124:38: replace default_csnotes_dir -> String with "xyzzy".into() in 1s build + 1s test
MISSED   src/config.rs:129:47: replace default_archive_threshold_weeks -> u32 with 0 in 1s build + 2s test
MISSED   src/config.rs:129:47: replace default_archive_threshold_weeks -> u32 with 1 in 1s build + 1s test
MISSED   src/config.rs:134:46: replace default_scan_ai_conversations -> bool with false in 1s build + 1s test
MISSED   src/config.rs:147:9: replace VaultConfig::save -> Result<()> with Ok(()) in 1s build + 1s test
MISSED   src/config.rs:157:9: replace VaultConfig::validate -> Result<()> with Ok(()) in 1s build + 1s test
MISSED   src/config.rs:184:9: replace VaultConfig::instruction_source_path -> PathBuf with Default::default() in 1s build + 1s test
MISSED   src/config.rs:192:9: replace VaultConfig::synthesis_md_path -> PathBuf with Default::default() in 1s build + 1s test
MISSED   src/config.rs:199:9: replace VaultConfig::report_schema_path -> PathBuf with Default::default() in 1s build + 1s test
MISSED   src/config.rs:211:5: replace find_vault_root -> Result<PathBuf> with Ok(Default::default()) in 1s build + 1s test
MISSED   src/config.rs:262:9: replace <impl std::fmt::Debug for FilenameFormat>::fmt -> std::fmt::Result with Ok(Default::default()) in 1s build + 1s test
MISSED   src/config.rs:323:9: replace FilenameFormat::as_str -> &str with "" in 1s build + 1s test
MISSED   src/config.rs:323:9: replace FilenameFormat::as_str -> &str with "xyzzy" in 1s build + 1s test
MISSED   src/config.rs:420:8: delete ! in tokenise in 1s build + 1s test
MISSED   src/config.rs:430:5: replace ensure_no_spaces -> Result<()> with Ok(()) in 1s build + 1s test
MISSED   src/flags.rs:32:9: replace FlagStore::load -> Result<Self> with Ok(Default::default()) in 1s build + 1s test
MISSED   src/flags.rs:32:12: delete ! in FlagStore::load in 1s build + 1s test
MISSED   src/flags.rs:44:9: replace FlagStore::save -> Result<()> with Ok(()) in 1s build + 1s test
MISSED   src/flags.rs:88:9: replace FlagStore::open_threads -> impl Iterator<Item =&StoredFlag> with ::std::iter::empty() in 1s build + 1s test
MISSED   src/flags.rs:90:32: replace && with || in FlagStore::open_threads in 1s build + 1s test
MISSED   src/flags.rs:95:9: replace FlagStore::open_for_path -> impl Iterator<Item =&'a StoredFlag>+'a with ::std::iter::empty() in 1s build + 1s test
MISSED   src/flags.rs:96:20: replace && with || in FlagStore::open_for_path in 1s build + 1s test
MISSED   src/flags.rs:96:41: replace == with != in FlagStore::open_for_path in 1s build + 1s test
MISSED   src/flags.rs:103:9: replace FlagStore::open_for_topic -> impl Iterator<Item =&'a StoredFlag>+'a with ::std::iter::empty() in 1s build + 1s test
MISSED   src/flags.rs:105:20: replace && with || in FlagStore::open_for_topic in 1s build + 1s test
MISSED   src/flags.rs:112:9: replace FlagStore::resolved_with_follow_up -> impl Iterator<Item =&StoredFlag> with ::std::iter::empty() in 1s build + 1s test
MISSED   src/flags.rs:114:33: replace && with || in FlagStore::resolved_with_follow_up in 1s build + 1s test
MISSED   src/flags.rs:114:25: delete ! in FlagStore::resolved_with_follow_up in 1s build + 1s test
MISSED   src/flags.rs:159:9: replace StoredFlag::display_kind -> &'static str with "" in 1s build + 1s test
MISSED   src/flags.rs:159:9: replace StoredFlag::display_kind -> &'static str with "xyzzy" in 1s build + 4s test
MISSED   src/frontmatter.rs:129:16: delete ! in NoteFrontmatter::merge_provenance in 1s build + 2s test
MISSED   src/frontmatter.rs:132:38: replace == with != in NoteFrontmatter::merge_provenance in 1s build + 2s test
MISSED   src/frontmatter.rs:147:9: replace NoteFrontmatter::touch with () in 1s build + 1s test
MISSED   src/frontmatter.rs:173:61: replace - with + in split_frontmatter in 1s build + 2s test
MISSED   src/frontmatter.rs:173:61: replace - with / in split_frontmatter in 1s build + 2s test
MISSED   src/manifest.rs:81:9: replace Manifest::flags_path_absolute -> PathBuf with Default::default() in 2s build + 2s test
MISSED   src/manifest.rs:86:9: replace Manifest::last_report_path -> PathBuf with Default::default() in 1s build + 2s test
MISSED   src/manifest.rs:91:9: replace Manifest::session_report_path -> PathBuf with Default::default() in 1s build + 2s test
MISSED   src/manifest.rs:363:9: replace <impl Drop for ManifestLock>::drop with () in 1s build + 2s test
MISSED   src/markdown.rs:88:13: delete match arm NodeValue::SoftBreak | NodeValue::LineBreak in collect_text in 1s build + 2s test
MISSED   src/markdown.rs:97:5: replace line_to_byte_offset -> usize with 0 in 1s build + 2s test
MISSED   src/markdown.rs:97:5: replace line_to_byte_offset -> usize with 1 in 1s build + 2s test
MISSED   src/markdown.rs:99:29: replace == with != in line_to_byte_offset in 1s build + 3s test
MISSED   src/markdown.rs:100:25: replace + with - in line_to_byte_offset in 1s build + 2s test
MISSED   src/markdown.rs:100:25: replace + with * in line_to_byte_offset in 2s build + 2s test
MISSED   src/obsidian.rs:104:9: replace EmbedTarget::is_block_anchor -> bool with true in 1s build + 2s test
MISSED   src/obsidian.rs:144:5: replace collect_all_block_ids -> Result<HashMap<String, String>> with Ok(HashMap::new()) in 1s build + 2s test
MISSED   src/obsidian.rs:144:5: replace collect_all_block_ids -> Result<HashMap<String, String>> with Ok(HashMap::from_iter([(String::new(), String::new())])) in 1s build + 2s test
MISSED   src/obsidian.rs:144:5: replace collect_all_block_ids -> Result<HashMap<String, String>> with Ok(HashMap::from_iter([(String::new(), "xyzzy".into())])) in 1s build + 2s test
MISSED   src/obsidian.rs:144:5: replace collect_all_block_ids -> Result<HashMap<String, String>> with Ok(HashMap::from_iter([("xyzzy".into(), String::new())])) in 1s build + 2s test
MISSED   src/obsidian.rs:144:5: replace collect_all_block_ids -> Result<HashMap<String, String>> with Ok(HashMap::from_iter([("xyzzy".into(), "xyzzy".into())])) in 1s build + 2s test
MISSED   src/obsidian.rs:149:62: replace == with != in collect_all_block_ids in 1s build + 2s test
MISSED   src/report.rs:99:9: replace Op::kind_str -> &'static str with "" in 1s build + 2s test
MISSED   src/report.rs:99:9: replace Op::kind_str -> &'static str with "xyzzy" in 1s build + 2s test
MISSED   src/report.rs:113:9: replace Op::is_indexing -> bool with true in 1s build + 2s test
MISSED   src/report.rs:113:9: replace Op::is_indexing -> bool with false in 1s build + 2s test
MISSED   src/report.rs:117:9: replace Op::is_structural -> bool with true in 1s build + 2s test
MISSED   src/report.rs:117:9: replace Op::is_structural -> bool with false in 1s build + 2s test
MISSED   src/report.rs:117:9: delete ! in Op::is_structural in 1s build + 2s test
MISSED   src/report.rs:238:9: replace FlagKind::is_actionable -> bool with true in 1s build + 2s test
MISSED   src/report.rs:242:9: replace FlagKind::is_thread -> bool with true in 1s build + 2s test
MISSED   src/report.rs:242:9: replace FlagKind::is_thread -> bool with false in 1s build + 4s test
MISSED   src/report.rs:246:9: replace FlagKind::tier_label -> &'static str with "" in 1s build + 2s test
MISSED   src/report.rs:246:9: replace FlagKind::tier_label -> &'static str with "xyzzy" in 1s build + 2s test
MISSED   src/ui.rs:12:5: replace init_color with () in 1s build + 2s test
MISSED   src/ui.rs:12:8: delete ! in init_color in 1s build + 2s test
MISSED   src/ui.rs:19:5: replace color_supported -> bool with true in 1s build + 2s test
MISSED   src/ui.rs:19:5: replace color_supported -> bool with false in 1s build + 2s test
MISSED   src/ui.rs:21:9: replace && with || in color_supported in 1s build + 2s test
MISSED   src/ui.rs:20:9: replace && with || in color_supported in 1s build + 2s test
MISSED   src/ui.rs:20:53: replace != with == in color_supported in 1s build + 2s test
MISSED   src/ui.rs:27:5: replace rainbow -> String with String::new() in 1s build + 2s test
MISSED   src/ui.rs:27:5: replace rainbow -> String with "xyzzy".into() in 1s build + 2s test
MISSED   src/ui.rs:27:8: delete ! in rainbow in 1s build + 2s test
MISSED   src/ui.rs:34:41: replace * with + in rainbow in 1s build + 2s test
MISSED   src/ui.rs:34:41: replace * with / in rainbow in 1s build + 2s test
MISSED   src/ui.rs:34:29: replace / with % in rainbow in 1s build + 2s test
MISSED   src/ui.rs:34:29: replace / with * in rainbow in 1s build + 2s test
MISSED   src/ui.rs:43:5: replace hsl_to_rgb -> (u8, u8, u8) with (0, 0, 0) in 1s build + 2s test
MISSED   src/ui.rs:43:5: replace hsl_to_rgb -> (u8, u8, u8) with (0, 0, 1) in 1s build + 2s test
MISSED   src/ui.rs:43:5: replace hsl_to_rgb -> (u8, u8, u8) with (0, 1, 0) in 1s build + 2s test
MISSED   src/ui.rs:43:5: replace hsl_to_rgb -> (u8, u8, u8) with (0, 1, 1) in 1s build + 2s test
MISSED   src/ui.rs:43:5: replace hsl_to_rgb -> (u8, u8, u8) with (1, 0, 0) in 1s build + 2s test
MISSED   src/ui.rs:43:5: replace hsl_to_rgb -> (u8, u8, u8) with (1, 0, 1) in 1s build + 2s test
MISSED   src/ui.rs:43:5: replace hsl_to_rgb -> (u8, u8, u8) with (1, 1, 0) in 1s build + 2s test
MISSED   src/ui.rs:43:5: replace hsl_to_rgb -> (u8, u8, u8) with (1, 1, 1) in 1s build + 2s test
MISSED   src/ui.rs:43:43: replace * with + in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:43:43: replace * with / in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:43:18: replace - with + in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:43:18: replace - with / in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:43:29: replace - with + in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:43:29: replace - with / in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:43:25: replace * with + in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:43:25: replace * with / in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:44:15: replace * with + in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:44:15: replace * with / in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:44:22: replace - with + in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:44:22: replace - with / in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:44:42: replace - with + in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:44:42: replace - with / in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:44:36: replace % with / in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:44:36: replace % with + in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:44:28: replace / with % in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:44:28: replace / with * in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:45:15: replace - with + in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:45:15: replace - with / in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:45:19: replace / with % in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:45:19: replace / with * in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:46:29: replace < with == in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:46:29: replace < with > in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:46:29: replace < with <= in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:48:17: replace < with == in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:48:17: replace < with > in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:48:17: replace < with <= in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:50:17: replace < with == in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:50:17: replace < with > in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:50:17: replace < with <= in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:52:17: replace < with == in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:52:17: replace < with > in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:52:17: replace < with <= in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:54:17: replace < with == in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:54:17: replace < with > in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:54:17: replace < with <= in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:60:19: replace * with + in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:60:19: replace * with / in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:60:14: replace + with - in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:60:14: replace + with * in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:61:19: replace * with + in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:61:19: replace * with / in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:61:14: replace + with - in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:61:14: replace + with * in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:62:19: replace * with + in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:62:19: replace * with / in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:62:14: replace + with - in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:62:14: replace + with * in hsl_to_rgb in 1s build + 2s test
MISSED   src/workspace.rs:34:5: replace workspace_base_dir -> Result<PathBuf> with Ok(Default::default()) in 1s build + 2s test
MISSED   src/workspace.rs:129:5: replace copy_instruction_files -> Result<()> with Ok(()) in 2s build + 2s test
MISSED   src/workspace.rs:167:5: replace xml_wrap -> String with String::new() in 1s build + 2s test
MISSED   src/workspace.rs:167:5: replace xml_wrap -> String with "xyzzy".into() in 1s build + 2s test
MISSED   src/workspace.rs:179:5: replace wrap_session_inputs -> Result<()> with Ok(()) in 2s build + 2s test
MISSED   src/workspace.rs:195:12: delete ! in wrap_session_inputs in 1s build + 2s test
MISSED   src/workspace.rs:200:13: delete match arm crate::manifest::RecordingKind::Transcript in wrap_session_inputs in 1s build + 2s test
MISSED   src/workspace.rs:201:13: delete match arm crate::manifest::RecordingKind::Summary in wrap_session_inputs in 1s build + 2s test
MISSED   src/workspace.rs:202:13: delete match arm crate::manifest::RecordingKind::Mindmap in wrap_session_inputs in 1s build + 2s test
MISSED   src/workspace.rs:215:12: delete ! in wrap_session_inputs in 1s build + 2s test
MISSED   src/workspace.rs:223:12: delete ! in wrap_session_inputs in 1s build + 2s test
MISSED   src/workspace.rs:238:13: delete match arm crate::manifest::ArtifactKind::Code in wrap_session_inputs in 1s build + 2s test
MISSED   src/workspace.rs:272:5: replace copy_sources_to_workspace -> Result<()> with Ok(()) in 1s build + 2s test
MISSED   src/workspace.rs:293:5: replace write_source_file -> Result<()> with Ok(()) in 1s build + 2s test
MISSED   src/workspace.rs:294:8: delete ! in write_source_file in 1s build + 2s test
MISSED   src/workspace.rs:384:5: replace render_session_md -> Result<()> with Ok(()) in 2s build + 2s test
MISSED   src/workspace.rs:431:45: replace == with != in render_session_md in 2s build + 2s test
MISSED   src/workspace.rs:487:16: delete ! in render_session_md in 1s build + 2s test
MISSED   src/workspace.rs:507:16: delete ! in render_session_md in 1s build + 2s test
MISSED   src/workspace.rs:516:16: delete ! in render_session_md in 1s build + 2s test
MISSED   src/workspace.rs:539:8: delete ! in render_session_md in 1s build + 2s test
MISSED   src/workspace.rs:575:21: delete ! in render_session_md in 3s build + 2s test
MISSED   src/workspace.rs:583:21: delete ! in render_session_md in 2s build + 2s test
MISSED   src/workspace.rs:587:43: replace && with || in render_session_md in 1s build + 2s test
MISSED   src/workspace.rs:620:5: replace take_snapshot -> Result<PathBuf> with Ok(Default::default()) in 1s build + 2s test
MISSED   src/workspace.rs:756:13: delete match arm (None, None) in rebuild_cross_embedded_in in 1s build + 2s test
MISSED   src/workspace.rs:757:13: delete match arm (Some(a), Some(b)) in rebuild_cross_embedded_in in 1s build + 2s test
MISSED   src/workspace.rs:757:37: replace == with != in rebuild_cross_embedded_in in 1s build + 2s test
MISSED   src/workspace.rs:812:5: replace copy_dir -> Result<()> with Ok(()) in 1s build + 2s test
MISSED   src/workspace.rs:831:5: replace make_readonly -> Result<()> with Ok(()) in 1s build + 2s test
MISSED   src/workspace.rs:838:5: replace sanitise -> String with String::new() in 1s build + 3s test
MISSED   src/workspace.rs:838:5: replace sanitise -> String with "xyzzy".into() in 2s build + 2s test
MISSED   src/workspace.rs:845:5: replace is_text_artifact_ext -> bool with true in 1s build + 3s test
MISSED   src/workspace.rs:845:5: replace is_text_artifact_ext -> bool with false in 1s build + 2s test
MISSED   src/ops/content.rs:84:5: replace execute_update_note -> Result<()> with Ok(()) in 2s build + 3s test
MISSED   src/ops/content.rs:86:8: delete ! in execute_update_note in 3s build + 4s test
MISSED   src/ops/structural.rs:287:70: replace == with != in execute_merge_topics in 2s build + 2s test
MISSED   src/ops/structural.rs:294:23: replace == with != in execute_merge_topics in 1s build + 3s test
MISSED   src/ops/structural.rs:305:49: replace && with || in execute_merge_topics in 2s build + 2s test
MISSED   src/ops/structural.rs:305:93: replace == with != in execute_merge_topics in 1s build + 2s test
MISSED   src/ops/structural.rs:472:29: replace != with == in execute_set_embed in 3s build + 2s test
MISSED   src/ops/structural.rs:476:28: replace != with == in execute_set_embed in 2s build + 2s test
MISSED   src/ops/structural.rs:506:45: replace && with || in move_topic_notes in 2s build + 3s test
MISSED   src/ops/structural.rs:556:40: replace || with && in rewrite_links in 2s build + 2s test
MISSED   src/ops/structural.rs:586:27: replace += with *= in rewrite_note_links in 1s build + 2s test
TIMEOUT  src/ops/structural.rs:612:40: replace + with - in replace_note_links in 2s build + 20s test
TIMEOUT  src/ops/structural.rs:612:40: replace + with * in replace_note_links in 3s build + 20s test
MISSED   src/commands/audit_cmd.rs:15:5: replace run -> Result<()> with Ok(()) in 3s build + 2s test
MISSED   src/commands/audit_cmd.rs:62:12: delete ! in run in 2s build + 3s test
MISSED   src/commands/audit_cmd.rs:67:22: replace > with == in run in 2s build + 3s test
MISSED   src/commands/audit_cmd.rs:67:22: replace > with < in run in 1s build + 3s test
MISSED   src/commands/audit_cmd.rs:67:22: replace > with >= in run in 2s build + 3s test
MISSED   src/commands/audit_cmd.rs:78:8: delete ! in run in 1s build + 3s test
MISSED   src/commands/audit_cmd.rs:82:8: delete ! in run in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:21:5: replace run -> Result<()> with Ok(()) in 3s build + 2s test
MISSED   src/commands/config_cmd.rs:45:44: replace != with == in run in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:46:40: replace == with != in run in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:106:5: replace run_migrate -> Result<()> with Ok(()) in 2s build + 4s test
MISSED   src/commands/config_cmd.rs:116:23: replace == with != in run_migrate in 2s build + 4s test
MISSED   src/commands/config_cmd.rs:211:8: delete ! in run_migrate in 3s build + 3s test
MISSED   src/commands/config_cmd.rs:223:16: delete ! in run_migrate in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:266:5: replace try_rename_in_path -> Option<String> with None in 2s build + 2s test
MISSED   src/commands/config_cmd.rs:266:5: replace try_rename_in_path -> Option<String> with Some(String::new()) in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:266:5: replace try_rename_in_path -> Option<String> with Some("xyzzy".into()) in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:268:8: delete ! in try_rename_in_path in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:274:25: replace match guard parent != Path::new("") with true in try_rename_in_path in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:274:25: replace match guard parent != Path::new("") with false in try_rename_in_path in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:274:32: replace != with == in try_rename_in_path in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:285:5: replace rename_in_path -> Result<String> with Ok(String::new()) in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:285:5: replace rename_in_path -> Result<String> with Ok("xyzzy".into()) in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:306:5: replace check_no_unregistered -> Result<()> with Ok(()) in 2s build + 2s test
MISSED   src/commands/config_cmd.rs:344:12: delete ! in check_no_unregistered in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:353:21: replace && with || in check_no_unregistered in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:353:65: replace == with != in check_no_unregistered in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:376:8: delete ! in check_no_unregistered in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:395:5: replace apply_set -> Result<()> with Ok(()) in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:396:9: delete match arm "filename_format" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:400:9: delete match arm "raw_dir" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:404:9: delete match arm "recordings_dir" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:408:9: delete match arm "require_recordings" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:415:9: delete match arm "artifacts_dir" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:419:9: delete match arm "sources_dir" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:423:9: delete match arm "default_backend" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:431:9: delete match arm "archive_threshold_weeks" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:436:9: delete match arm "agy_model" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:443:9: delete match arm "scan_ai_conversations" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:410:17: delete match arm "true" | "1" | "yes" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:411:17: delete match arm "false" | "0" | "no" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:425:17: delete match arm "claude" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:426:17: delete match arm "agy" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:427:17: delete match arm "mock" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:445:17: delete match arm "true" | "1" | "yes" in apply_set in 3s build + 5s test
MISSED   src/commands/config_cmd.rs:446:17: delete match arm "false" | "0" | "no" in apply_set in 2s build + 4s test
MISSED   src/commands/diff.rs:16:5: replace run -> Result<()> with Ok(()) in 2s build + 3s test
MISSED   src/commands/diff.rs:25:12: delete ! in run in 2s build + 3s test
MISSED   src/commands/diff.rs:35:12: delete ! in run in 2s build + 3s test
MISSED   src/commands/diff.rs:67:8: delete ! in run in 2s build + 4s test
MISSED   src/commands/diff.rs:72:16: delete ! in run in 2s build + 4s test
MISSED   src/commands/diff.rs:81:8: delete ! in run in 2s build + 3s test
MISSED   src/commands/diff.rs:86:16: delete ! in run in 2s build + 5s test
MISSED   src/commands/diff.rs:92:8: delete ! in run in 2s build + 3s test
MISSED   src/commands/diff.rs:107:8: delete ! in run in 2s build + 4s test
MISSED   src/commands/diff.rs:118:8: delete ! in run in 2s build + 3s test
MISSED   src/commands/diff.rs:125:37: replace && with || in run in 4s build + 4s test
MISSED   src/commands/diff.rs:136:5: replace resolve_session_id -> Result<String> with Ok(String::new()) in 4s build + 4s test
MISSED   src/commands/diff.rs:136:5: replace resolve_session_id -> Result<String> with Ok("xyzzy".into()) in 13s build + 7s test
MISSED   src/commands/diff.rs:147:9: delete match arm 0 in resolve_session_id in 3s build + 3s test
MISSED   src/commands/diff.rs:148:9: delete match arm 1 in resolve_session_id in 2s build + 3s test
MISSED   src/commands/extract.rs:27:5: replace run -> Result<()> with Ok(()) in 2s build + 3s test
MISSED   src/commands/extract.rs:33:12: delete ! in run in 4s build + 3s test
MISSED   src/commands/extract.rs:42:39: replace == with != in run in 2s build + 3s test
MISSED   src/commands/extract.rs:72:12: delete ! in run in 2s build + 3s test
MISSED   src/commands/extract.rs:114:9: replace ExtractKind::label -> &'static str with "" in 2s build + 3s test
MISSED   src/commands/extract.rs:114:9: replace ExtractKind::label -> &'static str with "xyzzy" in 2s build + 4s test
MISSED   src/commands/extract.rs:130:67: replace || with && in extract_from in 2s build + 3s test
MISSED   src/commands/extract.rs:145:25: replace + with * in extract_from in 2s build + 6s test
MISSED   src/commands/extract.rs:151:25: replace + with * in extract_from in 2s build + 4s test
MISSED   src/commands/extract.rs:157:25: replace + with * in extract_from in 2s build + 3s test
MISSED   src/commands/extract.rs:170:66: replace || with && in is_action in 2s build + 5s test
MISSED   src/commands/extract.rs:209:81: replace || with && in is_question in 2s build + 5s test
MISSED   src/commands/extract.rs:209:69: replace || with && in is_question in 2s build + 7s test
MISSED   src/commands/extract.rs:214:9: replace || with && in is_question in 2s build + 3s test
MISSED   src/commands/extract.rs:213:9: replace || with && in is_question in 2s build + 3s test
MISSED   src/commands/extract.rs:218:5: replace clean_question -> String with String::new() in 2s build + 3s test
MISSED   src/commands/extract.rs:218:5: replace clean_question -> String with "xyzzy".into() in 2s build + 4s test
MISSED   src/commands/extract.rs:234:5: replace render_extract -> String with String::new() in 3s build + 4s test
MISSED   src/commands/extract.rs:234:5: replace render_extract -> String with "xyzzy".into() in 3s build + 5s test
MISSED   src/commands/extract.rs:238:62: replace == with != in render_extract in 3s build + 5s test
MISSED   src/commands/extract.rs:253:5: replace capitalise -> String with String::new() in 2s build + 4s test
MISSED   src/commands/extract.rs:253:5: replace capitalise -> String with "xyzzy".into() in 2s build + 4s test
MISSED   src/commands/extract.rs:261:5: replace relative_path -> String with String::new() in 2s build + 4s test
MISSED   src/commands/extract.rs:261:5: replace relative_path -> String with "xyzzy".into() in 2s build + 4s test
MISSED   src/commands/flags_cmd.rs:14:5: replace run -> Result<()> with Ok(()) in 2s build + 3s test
MISSED   src/commands/flags_cmd.rs:24:38: replace && with || in run in 2s build + 5s test
MISSED   src/commands/flags_cmd.rs:24:61: replace || with && in run in 4s build + 4s test
MISSED   src/commands/flags_cmd.rs:24:64: delete ! in run in 3s build + 5s test
MISSED   src/commands/flags_cmd.rs:43:40: replace == with != in run in 3s build + 4s test
MISSED   src/commands/flags_cmd.rs:45:20: delete ! in run in 3s build + 5s test
MISSED   src/commands/flags_cmd.rs:55:16: delete ! in run in 3s build + 4s test
MISSED   src/commands/flags_cmd.rs:63:57: replace == with != in run in 6s build + 6s test
MISSED   src/commands/init.rs:23:5: replace run -> Result<()> with Ok(()) in 4s build + 7s test
MISSED   src/commands/init.rs:46:29: delete ! in run in 6s build + 3s test
MISSED   src/commands/init.rs:62:9: delete match arm "agy" in run in 2s build + 5s test
MISSED   src/commands/init.rs:67:9: delete match arm AiBackend::Agy in run in 2s build + 3s test
MISSED   src/commands/init.rs:122:8: delete ! in run in 2s build + 3s test
MISSED   src/commands/init.rs:153:5: replace run_instructions_only -> Result<()> with Ok(()) in 3s build + 5s test
MISSED   src/commands/init.rs:170:5: replace write_instruction_files -> Result<()> with Ok(()) in 4s build + 5s test
MISSED   src/commands/init.rs:177:26: replace == with != in write_instruction_files in 4s build + 5s test
MISSED   src/commands/init.rs:190:5: replace write_if_absent -> Result<()> with Ok(()) in 4s build + 6s test
MISSED   src/commands/init.rs:823:5: replace prompt_courses -> Result<Vec<String>> with Ok(vec![]) in 2s build + 4s test
MISSED   src/commands/init.rs:823:5: replace prompt_courses -> Result<Vec<String>> with Ok(vec![String::new()]) in 2s build + 5s test
MISSED   src/commands/init.rs:823:5: replace prompt_courses -> Result<Vec<String>> with Ok(vec!["xyzzy".into()]) in 3s build + 4s test
MISSED   src/commands/init.rs:852:5: replace prompt_bool -> Result<bool> with Ok(true) in 4s build + 5s test
MISSED   src/commands/init.rs:852:5: replace prompt_bool -> Result<bool> with Ok(false) in 4s build + 7s test
MISSED   src/commands/init.rs:857:9: delete match arm "y" | "yes" in prompt_bool in 7s build + 8s test
MISSED   src/commands/init.rs:858:9: delete match arm "n" | "no" in prompt_bool in 7s build + 7s test
MISSED   src/commands/init.rs:864:5: replace prompt -> Result<String> with Ok(String::new()) in 4s build + 5s test
MISSED   src/commands/init.rs:864:5: replace prompt -> Result<String> with Ok("xyzzy".into()) in 3s build + 5s test
MISSED   src/commands/process.rs:72:17: replace && with || in run in 3s build + 6s test
MISSED   src/commands/process.rs:71:17: replace && with || in run in 4s build + 5s test
MISSED   src/commands/process.rs:72:20: delete ! in run in 4s build + 6s test
MISSED   src/commands/process.rs:128:24: delete ! in run in 4s build + 5s test
MISSED   src/commands/process.rs:240:13: delete match arm Op::RenameTopic(o) in run_teardown in 2s build + 5s test
MISSED   src/commands/process.rs:243:13: delete match arm Op::MoveAtomic(o) in run_teardown in 3s build + 6s test
MISSED   src/commands/process.rs:246:13: delete match arm Op::PromoteAtomic(o) in run_teardown in 4s build + 8s test
MISSED   src/commands/process.rs:249:13: delete match arm Op::DemoteTopic(o) in run_teardown in 4s build + 7s test
MISSED   src/commands/process.rs:252:13: delete match arm Op::MergeTopics(o) in run_teardown in 5s build + 7s test
MISSED   src/commands/process.rs:255:13: delete match arm Op::SplitTopic(o) in run_teardown in 5s build + 6s test
MISSED   src/commands/process.rs:258:13: delete match arm Op::SetEmbed(o) in run_teardown in 5s build + 5s test
MISSED   src/commands/process.rs:274:13: delete match arm Op::UpdateNote(op) in run_teardown in 10s build + 8s test
MISSED   src/commands/process.rs:350:16: replace > with == in run_teardown in 2s build + 5s test
MISSED   src/commands/process.rs:350:16: replace > with < in run_teardown in 4s build + 5s test
MISSED   src/commands/process.rs:350:16: replace > with >= in run_teardown in 5s build + 8s test
MISSED   src/commands/process.rs:368:8: delete ! in resolve_scope in 4s build + 9s test
MISSED   src/commands/process.rs:385:5: replace expand_source_ids -> Result<Vec<String>> with Ok(vec![]) in 5s build + 7s test
MISSED   src/commands/process.rs:385:5: replace expand_source_ids -> Result<Vec<String>> with Ok(vec![String::new()]) in 5s build + 6s test
MISSED   src/commands/process.rs:385:5: replace expand_source_ids -> Result<Vec<String>> with Ok(vec!["xyzzy".into()]) in 4s build + 10s test
MISSED   src/commands/process.rs:439:13: delete match arm 0 in resolve_session_id in 4s build + 6s test
MISSED   src/commands/process.rs:451:9: delete match arm 0 in resolve_session_id in 4s build + 6s test
MISSED   src/commands/process.rs:799:9: delete match arm "p" in prompt_no_recording in 4s build + 6s test
MISSED   src/commands/process.rs:800:9: delete match arm "q" in prompt_no_recording in 5s build + 9s test
MISSED   src/commands/recover.rs:49:8: delete ! in run in 5s build + 8s test
MISSED   src/commands/recover.rs:133:9: delete match arm "r" | "resume" in prompt_choice in 4s build + 7s test
MISSED   src/commands/recover.rs:139:5: replace prompt_discard_only -> Result<()> with Ok(()) in 5s build + 6s test
MISSED   src/commands/recover.rs:144:9: delete match arm "y" | "Y" in prompt_discard_only in 5s build + 10s test
MISSED   src/commands/reconcile.rs:148:9: replace && with || in run_for_vault in 4s build + 10s test
MISSED   src/commands/reconcile.rs:147:9: replace && with || in run_for_vault in 4s build + 8s test
MISSED   src/commands/reconcile.rs:146:9: replace && with || in run_for_vault in 5s build + 8s test
MISSED   src/commands/reconcile.rs:145:9: replace && with || in run_for_vault in 5s build + 8s test
MISSED   src/commands/reconcile.rs:151:12: delete ! in run_for_vault in 5s build + 9s test
MISSED   src/commands/reconcile.rs:169:12: delete ! in run_for_vault in 5s build + 9s test
MISSED   src/commands/reconcile.rs:178:20: replace && with || in run_for_vault in 7s build + 17s test
MISSED   src/commands/reconcile.rs:178:23: delete ! in run_for_vault in 11s build + 13s test
MISSED   src/commands/reconcile.rs:334:58: replace == with != in scan_recordings_dir in 5s build + 10s test
MISSED   src/commands/reconcile.rs:350:5: replace build_date -> Option<NaiveDate> with Some(Default::default()) in 5s build + 14s test
MISSED   src/commands/reconcile.rs:363:5: replace handle_spaces -> Result<Option<PathBuf>> with Ok(None) in 5s build + 10s test
MISSED   src/commands/reconcile.rs:367:8: delete ! in handle_spaces in 4s build + 9s test
MISSED   src/commands/reconcile.rs:373:40: replace == with != in handle_spaces in 4s build + 9s test
MISSED   src/commands/reconcile.rs:388:9: delete match arm "transcript" in recording_kind in 5s build + 10s test
MISSED   src/commands/reconcile.rs:389:9: delete match arm "summary" in recording_kind in 6s build + 10s test
MISSED   src/commands/reconcile.rs:390:9: delete match arm "mindmap" in recording_kind in 7s build + 12s test
MISSED   src/commands/reconcile.rs:391:14: replace match guard q.len() == 1 && q.chars().next().map_or(false, |c| c.is_ascii_lowercase()) with true in recording_kind in 5s build + 10s test
MISSED   src/commands/reconcile.rs:391:14: replace match guard q.len() == 1 && q.chars().next().map_or(false, |c| c.is_ascii_lowercase()) with false in recording_kind in 6s build + 12s test
MISSED   src/commands/reconcile.rs:391:27: replace && with || in recording_kind in 5s build + 10s test
MISSED   src/commands/reconcile.rs:391:22: replace == with != in recording_kind in 5s build + 10s test
MISSED   src/commands/reconcile.rs:600:13: delete match arm (_, Some(stem)) in scan_sources_dir in 8s build + 12s test
MISSED   src/commands/reconcile.rs:597:43: replace match guard !parent.is_empty() with true in scan_sources_dir in 5s build + 12s test
MISSED   src/commands/reconcile.rs:1001:5: replace notify with () in 5s build + 11s test
MISSED   src/commands/status.rs:11:5: replace run -> Result<()> with Ok(()) in 7s build + 11s test
MISSED   src/commands/status.rs:42:35: replace == with != in run in 6s build + 11s test
MISSED   src/commands/status.rs:47:30: replace == with != in run in 7s build + 12s test
MISSED   src/commands/status.rs:63:29: delete ! in run in 5s build + 11s test
MISSED   src/commands/status.rs:88:8: delete ! in run in 6s build + 12s test
MISSED   src/commands/status.rs:101:8: delete ! in run in 6s build + 12s test
MISSED   src/commands/status.rs:113:45: replace > with == in run in 6s build + 17s test
MISSED   src/commands/status.rs:113:45: replace > with < in run in 6s build + 12s test
MISSED   src/commands/status.rs:113:45: replace > with >= in run in 6s build + 15s test
MISSED   src/commands/status.rs:145:35: replace == with != in run in 6s build + 12s test
MISSED   src/commands/status.rs:150:20: replace > with == in run in 5s build + 13s test
MISSED   src/commands/status.rs:150:20: replace > with < in run in 8s build + 13s test
MISSED   src/commands/status.rs:150:20: replace > with >= in run in 6s build + 12s test
MISSED   src/commands/status.rs:166:24: replace > with == in run in 6s build + 12s test
MISSED   src/commands/status.rs:166:24: replace > with < in run in 6s build + 12s test
MISSED   src/commands/status.rs:166:24: replace > with >= in run in 6s build + 16s test
784 mutants tested in 84m: 413 missed, 324 caught, 45 unviable, 2 timeouts
