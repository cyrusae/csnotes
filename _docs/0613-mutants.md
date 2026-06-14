Found 811 mutants to test
ok       Unmutated baseline in 15s build + 2s test
MISSED   src/audit.rs:198:64: replace == with != in invariant_suite in 2s build + 1s test
MISSED   src/audit.rs:406:20: delete ! in reindex in 1s build + 1s test
MISSED   src/audit.rs:409:57: replace && with || in reindex in 1s build + 1s test
MISSED   src/audit.rs:409:39: replace == with != in reindex in 1s build + 1s test
MISSED   src/audit.rs:409:67: replace == with != in reindex in 1s build + 1s test
MISSED   src/audit.rs:416:32: replace > with >= in reindex in 1s build + 2s test
MISSED   src/audit.rs:432:61: replace && with || in reindex in 1s build + 1s test
MISSED   src/audit.rs:432:34: replace > with == in reindex in 1s build + 1s test
MISSED   src/audit.rs:432:34: replace > with < in reindex in 1s build + 1s test
MISSED   src/audit.rs:432:34: replace > with >= in reindex in 1s build + 1s test
MISSED   src/audit.rs:501:40: replace && with || in check_embed_line_present in 1s build + 1s test
MISSED   src/backend.rs:61:5: replace run_interactive -> std::io::Result<std::process::ExitStatus> with Ok(Default::default()) in 1s build + 1s test
MISSED   src/backend.rs:116:9: replace <impl BackendLauncher for ClaudeBackend>::launch -> Result<()> with Ok(()) in 1s build + 1s test
MISSED   src/backend.rs:125:12: delete ! in <impl BackendLauncher for ClaudeBackend>::launch in 1s build + 2s test
MISSED   src/backend.rs:135:9: replace <impl BackendLauncher for ClaudeBackend>::backend_name -> &'static str with "" in 1s build + 1s test
MISSED   src/backend.rs:135:9: replace <impl BackendLauncher for ClaudeBackend>::backend_name -> &'static str with "xyzzy" in 1s build + 1s test
MISSED   src/backend.rs:155:9: replace <impl BackendLauncher for AgyBackend>::launch -> Result<()> with Ok(()) in 1s build + 1s test
MISSED   src/backend.rs:180:12: delete ! in <impl BackendLauncher for AgyBackend>::launch in 1s build + 1s test
MISSED   src/backend.rs:190:9: replace <impl BackendLauncher for AgyBackend>::backend_name -> &'static str with "" in 1s build + 1s test
MISSED   src/backend.rs:190:9: replace <impl BackendLauncher for AgyBackend>::backend_name -> &'static str with "xyzzy" in 1s build + 1s test
MISSED   src/backend.rs:252:9: replace <impl BackendLauncher for MockBackend>::backend_name -> &'static str with "" in 2s build + 1s test
MISSED   src/backend.rs:252:9: replace <impl BackendLauncher for MockBackend>::backend_name -> &'static str with "xyzzy" in 1s build + 1s test
MISSED   src/config.rs:24:9: replace <impl std::fmt::Display for AiBackend>::fmt -> std::fmt::Result with Ok(Default::default()) in 2s build + 1s test
MISSED   src/config.rs:182:9: replace VaultConfig::save -> Result<()> with Ok(()) in 1s build + 1s test
MISSED   src/config.rs:190:9: replace VaultConfig::validate -> Result<()> with Ok(()) in 1s build + 1s test
MISSED   src/config.rs:219:9: replace VaultConfig::instruction_source_path -> PathBuf with Default::default() in 1s build + 1s test
MISSED   src/config.rs:230:9: replace VaultConfig::synthesis_md_path -> PathBuf with Default::default() in 1s build + 1s test
MISSED   src/config.rs:237:9: replace VaultConfig::report_schema_path -> PathBuf with Default::default() in 1s build + 1s test
MISSED   src/config.rs:249:5: replace find_vault_root -> Result<PathBuf> with Ok(Default::default()) in 1s build + 1s test
MISSED   src/config.rs:296:9: replace <impl std::fmt::Debug for FilenameFormat>::fmt -> std::fmt::Result with Ok(Default::default()) in 1s build + 1s test
MISSED   src/config.rs:362:9: replace FilenameFormat::as_str -> &str with "" in 1s build + 1s test
MISSED   src/config.rs:362:9: replace FilenameFormat::as_str -> &str with "xyzzy" in 1s build + 1s test
MISSED   src/config.rs:459:8: delete ! in tokenise in 1s build + 1s test
MISSED   src/flags.rs:32:9: replace FlagStore::load -> Result<Self> with Ok(Default::default()) in 1s build + 1s test
MISSED   src/flags.rs:32:12: delete ! in FlagStore::load in 1s build + 1s test
MISSED   src/flags.rs:44:9: replace FlagStore::save -> Result<()> with Ok(()) in 1s build + 1s test
MISSED   src/frontmatter.rs:129:16: delete ! in NoteFrontmatter::merge_provenance in 1s build + 1s test
MISSED   src/frontmatter.rs:132:38: replace == with != in NoteFrontmatter::merge_provenance in 1s build + 1s test
MISSED   src/manifest.rs:364:9: replace <impl Drop for ManifestLock>::drop with () in 1s build + 2s test
MISSED   src/markdown.rs:92:13: delete match arm NodeValue::SoftBreak | NodeValue::LineBreak in collect_text in 1s build + 2s test
MISSED   src/ui.rs:12:5: replace init_color with () in 1s build + 2s test
MISSED   src/ui.rs:12:8: delete ! in init_color in 1s build + 2s test
MISSED   src/ui.rs:19:5: replace color_supported -> bool with false in 1s build + 2s test
MISSED   src/ui.rs:21:9: replace && with || in color_supported in 1s build + 2s test
MISSED   src/ui.rs:20:53: replace != with == in color_supported in 1s build + 2s test
MISSED   src/ui.rs:34:41: replace * with + in rainbow in 1s build + 2s test
MISSED   src/ui.rs:34:41: replace * with / in rainbow in 1s build + 2s test
MISSED   src/ui.rs:34:29: replace / with % in rainbow in 1s build + 2s test
MISSED   src/ui.rs:34:29: replace / with * in rainbow in 1s build + 2s test
MISSED   src/ui.rs:43:43: replace * with + in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:43:43: replace * with / in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:43:18: replace - with + in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:46:29: replace < with <= in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:48:17: replace < with <= in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:50:17: replace < with <= in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:52:17: replace < with <= in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:54:17: replace < with <= in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:60:14: replace + with - in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:61:14: replace + with - in hsl_to_rgb in 1s build + 2s test
MISSED   src/ui.rs:62:14: replace + with - in hsl_to_rgb in 1s build + 2s test
MISSED   src/workspace.rs:35:5: replace workspace_base_dir -> Result<PathBuf> with Ok(Default::default()) in 2s build + 2s test
MISSED   src/workspace.rs:148:5: replace copy_instruction_files -> Result<()> with Ok(()) in 2s build + 2s test
MISSED   src/workspace.rs:198:5: replace wrap_session_inputs -> Result<()> with Ok(()) in 2s build + 2s test
MISSED   src/workspace.rs:218:12: delete ! in wrap_session_inputs in 2s build + 2s test
MISSED   src/workspace.rs:223:13: delete match arm crate::manifest::RecordingKind::Transcript in wrap_session_inputs in 1s build + 2s test
MISSED   src/workspace.rs:224:13: delete match arm crate::manifest::RecordingKind::Summary in wrap_session_inputs in 1s build + 2s test
MISSED   src/workspace.rs:225:13: delete match arm crate::manifest::RecordingKind::Mindmap in wrap_session_inputs in 1s build + 2s test
MISSED   src/workspace.rs:238:12: delete ! in wrap_session_inputs in 1s build + 2s test
MISSED   src/workspace.rs:246:12: delete ! in wrap_session_inputs in 1s build + 2s test
MISSED   src/workspace.rs:261:13: delete match arm crate::manifest::ArtifactKind::Code in wrap_session_inputs in 2s build + 2s test
MISSED   src/workspace.rs:410:8: delete ! in render_sidecar_md in 2s build + 2s test
MISSED   src/workspace.rs:427:12: delete ! in render_sidecar_md in 1s build + 2s test
MISSED   src/workspace.rs:466:24: delete ! in write_sources_index in 1s build + 2s test
MISSED   src/workspace.rs:524:5: replace render_session_md -> Result<()> with Ok(()) in 2s build + 2s test
MISSED   src/workspace.rs:577:45: replace == with != in render_session_md in 2s build + 2s test
MISSED   src/workspace.rs:632:16: delete ! in render_session_md in 1s build + 2s test
MISSED   src/workspace.rs:652:16: delete ! in render_session_md in 1s build + 2s test
MISSED   src/workspace.rs:663:16: delete ! in render_session_md in 1s build + 2s test
MISSED   src/workspace.rs:686:8: delete ! in render_session_md in 1s build + 2s test
MISSED   src/workspace.rs:724:21: delete ! in render_session_md in 1s build + 2s test
MISSED   src/workspace.rs:734:21: delete ! in render_session_md in 1s build + 2s test
MISSED   src/workspace.rs:740:43: replace && with || in render_session_md in 1s build + 2s test
MISSED   src/workspace.rs:772:5: replace take_snapshot -> Result<PathBuf> with Ok(Default::default()) in 1s build + 2s test
MISSED   src/workspace.rs:911:13: delete match arm (None, None) in rebuild_cross_embedded_in in 2s build + 2s test
MISSED   src/workspace.rs:912:13: delete match arm (Some(a), Some(b)) in rebuild_cross_embedded_in in 1s build + 2s test
MISSED   src/workspace.rs:912:37: replace == with != in rebuild_cross_embedded_in in 2s build + 2s test
MISSED   src/workspace.rs:967:5: replace copy_dir -> Result<()> with Ok(()) in 2s build + 2s test
MISSED   src/workspace.rs:986:5: replace make_readonly -> Result<()> with Ok(()) in 2s build + 2s test
MISSED   src/workspace.rs:993:5: replace sanitise -> String with String::new() in 2s build + 2s test
MISSED   src/workspace.rs:993:5: replace sanitise -> String with "xyzzy".into() in 1s build + 2s test
MISSED   src/workspace.rs:1000:5: replace is_text_artifact_ext -> bool with true in 1s build + 3s test
MISSED   src/workspace.rs:1000:5: replace is_text_artifact_ext -> bool with false in 1s build + 2s test
MISSED   src/ops/content.rs:82:5: replace execute_update_note -> Result<()> with Ok(()) in 2s build + 2s test
MISSED   src/ops/content.rs:84:8: delete ! in execute_update_note in 1s build + 2s test
MISSED   src/ops/structural.rs:364:68: replace == with != in execute_merge_topics in 1s build + 2s test
MISSED   src/ops/structural.rs:382:49: replace && with || in execute_merge_topics in 2s build + 2s test
MISSED   src/ops/structural.rs:382:91: replace == with != in execute_merge_topics in 2s build + 2s test
TIMEOUT  src/ops/structural.rs:680:40: replace + with - in replace_note_links in 1s build + 20s test
TIMEOUT  src/ops/structural.rs:680:40: replace + with * in replace_note_links in 3s build + 20s test
MISSED   src/ops/structural.rs:781:38: replace != with == in relink_raw_notes in 2s build + 2s test
MISSED   src/ops/structural.rs:790:37: replace != with == in relink_raw_notes in 2s build + 3s test
MISSED   src/commands/audit_cmd.rs:15:5: replace run -> Result<()> with Ok(()) in 2s build + 2s test
MISSED   src/commands/audit_cmd.rs:62:12: delete ! in run in 2s build + 3s test
MISSED   src/commands/audit_cmd.rs:67:22: replace > with == in run in 2s build + 3s test
MISSED   src/commands/audit_cmd.rs:67:22: replace > with < in run in 2s build + 3s test
MISSED   src/commands/audit_cmd.rs:67:22: replace > with >= in run in 2s build + 3s test
MISSED   src/commands/audit_cmd.rs:78:8: delete ! in run in 2s build + 3s test
MISSED   src/commands/audit_cmd.rs:82:8: delete ! in run in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:21:5: replace run -> Result<()> with Ok(()) in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:45:44: replace != with == in run in 3s build + 3s test
MISSED   src/commands/config_cmd.rs:46:40: replace == with != in run in 3s build + 4s test
MISSED   src/commands/config_cmd.rs:106:5: replace run_migrate -> Result<()> with Ok(()) in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:116:23: replace == with != in run_migrate in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:211:8: delete ! in run_migrate in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:223:16: delete ! in run_migrate in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:274:25: replace match guard parent != Path::new("") with true in try_rename_in_path in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:306:5: replace check_no_unregistered -> Result<()> with Ok(()) in 2s build + 2s test
MISSED   src/commands/config_cmd.rs:346:12: delete ! in check_no_unregistered in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:353:49: replace && with || in check_no_unregistered in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:353:91: replace == with != in check_no_unregistered in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:378:8: delete ! in check_no_unregistered in 4s build + 3s test
MISSED   src/commands/config_cmd.rs:402:9: delete match arm "raw_dir" in apply_set in 2s build + 2s test
MISSED   src/commands/config_cmd.rs:406:9: delete match arm "recordings_dir" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:417:9: delete match arm "artifacts_dir" in apply_set in 2s build + 3s test
MISSED   src/commands/config_cmd.rs:421:9: delete match arm "sources_dir" in apply_set in 2s build + 4s test
MISSED   src/commands/diff.rs:16:5: replace run -> Result<()> with Ok(()) in 2s build + 2s test
MISSED   src/commands/diff.rs:25:12: delete ! in run in 2s build + 4s test
MISSED   src/commands/diff.rs:35:12: delete ! in run in 2s build + 4s test
MISSED   src/commands/diff.rs:85:8: delete ! in run in 2s build + 3s test
MISSED   src/commands/diff.rs:90:16: delete ! in run in 2s build + 4s test
MISSED   src/commands/diff.rs:99:8: delete ! in run in 2s build + 4s test
MISSED   src/commands/diff.rs:104:16: delete ! in run in 2s build + 4s test
MISSED   src/commands/diff.rs:110:8: delete ! in run in 2s build + 4s test
MISSED   src/commands/diff.rs:129:8: delete ! in run in 2s build + 4s test
MISSED   src/commands/diff.rs:140:8: delete ! in run in 2s build + 4s test
MISSED   src/commands/diff.rs:147:37: replace && with || in run in 2s build + 3s test
MISSED   src/commands/diff.rs:169:9: delete match arm 0 in resolve_session_id in 2s build + 4s test
MISSED   src/commands/extract.rs:28:5: replace run -> Result<()> with Ok(()) in 2s build + 4s test
MISSED   src/commands/extract.rs:34:12: delete ! in run in 2s build + 4s test
MISSED   src/commands/extract.rs:43:39: replace == with != in run in 2s build + 4s test
MISSED   src/commands/extract.rs:73:12: delete ! in run in 2s build + 4s test
MISSED   src/commands/extract.rs:115:9: replace ExtractKind::label -> &'static str with "" in 2s build + 4s test
MISSED   src/commands/extract.rs:115:9: replace ExtractKind::label -> &'static str with "xyzzy" in 2s build + 4s test
MISSED   src/commands/extract.rs:131:65: replace || with && in extract_from in 2s build + 4s test
MISSED   src/commands/extract.rs:146:25: replace + with * in extract_from in 2s build + 4s test
MISSED   src/commands/extract.rs:152:25: replace + with * in extract_from in 2s build + 3s test
MISSED   src/commands/extract.rs:158:25: replace + with * in extract_from in 2s build + 4s test
MISSED   src/commands/extract.rs:169:62: replace || with && in is_action in 2s build + 4s test
MISSED   src/commands/extract.rs:230:81: replace || with && in is_question in 2s build + 4s test
MISSED   src/commands/extract.rs:230:69: replace || with && in is_question in 4s build + 4s test
MISSED   src/commands/extract.rs:235:9: replace || with && in is_question in 2s build + 3s test
MISSED   src/commands/extract.rs:234:9: replace || with && in is_question in 2s build + 4s test
MISSED   src/commands/extract.rs:239:5: replace clean_question -> String with String::new() in 2s build + 4s test
MISSED   src/commands/extract.rs:239:5: replace clean_question -> String with "xyzzy".into() in 2s build + 4s test
MISSED   src/commands/extract.rs:255:5: replace render_extract -> String with String::new() in 5s build + 6s test
MISSED   src/commands/extract.rs:255:5: replace render_extract -> String with "xyzzy".into() in 3s build + 5s test
MISSED   src/commands/extract.rs:263:62: replace == with != in render_extract in 4s build + 5s test
MISSED   src/commands/extract.rs:278:5: replace capitalise -> String with String::new() in 4s build + 5s test
MISSED   src/commands/extract.rs:278:5: replace capitalise -> String with "xyzzy".into() in 3s build + 4s test
MISSED   src/commands/extract.rs:286:5: replace relative_path -> String with String::new() in 3s build + 5s test
MISSED   src/commands/extract.rs:286:5: replace relative_path -> String with "xyzzy".into() in 3s build + 5s test
MISSED   src/commands/flags_cmd.rs:21:5: replace run -> Result<()> with Ok(()) in 3s build + 7s test
MISSED   src/commands/flags_cmd.rs:31:38: replace && with || in run in 6s build + 4s test
MISSED   src/commands/flags_cmd.rs:31:61: replace || with && in run in 2s build + 5s test
MISSED   src/commands/flags_cmd.rs:31:64: delete ! in run in 2s build + 5s test
MISSED   src/commands/flags_cmd.rs:50:40: replace == with != in run in 5s build + 5s test
MISSED   src/commands/flags_cmd.rs:52:20: delete ! in run in 3s build + 5s test
MISSED   src/commands/flags_cmd.rs:62:16: delete ! in run in 3s build + 5s test
MISSED   src/commands/flags_cmd.rs:70:57: replace == with != in run in 3s build + 5s test
MISSED   src/commands/init.rs:23:5: replace run -> Result<()> with Ok(()) in 4s build + 5s test
MISSED   src/commands/init.rs:45:29: delete ! in run in 4s build + 5s test
MISSED   src/commands/init.rs:61:9: delete match arm "agy" in run in 3s build + 5s test
MISSED   src/commands/init.rs:66:9: delete match arm AiBackend::Agy in run in 4s build + 5s test
MISSED   src/commands/init.rs:122:8: delete ! in run in 7s build + 5s test
MISSED   src/commands/init.rs:153:5: replace run_instructions_only -> Result<()> with Ok(()) in 4s build + 5s test
MISSED   src/commands/init.rs:173:5: replace write_instruction_files -> Result<()> with Ok(()) in 4s build + 5s test
MISSED   src/commands/init.rs:180:26: replace == with != in write_instruction_files in 4s build + 5s test
MISSED   src/commands/init.rs:193:5: replace write_if_absent -> Result<()> with Ok(()) in 3s build + 5s test
MISSED   src/commands/init.rs:831:5: replace prompt_courses -> Result<Vec<String>> with Ok(vec![]) in 2s build + 5s test
MISSED   src/commands/init.rs:831:5: replace prompt_courses -> Result<Vec<String>> with Ok(vec![String::new()]) in 4s build + 5s test
MISSED   src/commands/init.rs:831:5: replace prompt_courses -> Result<Vec<String>> with Ok(vec!["xyzzy".into()]) in 5s build + 5s test
MISSED   src/commands/init.rs:857:5: replace prompt_bool -> Result<bool> with Ok(true) in 4s build + 5s test
MISSED   src/commands/init.rs:857:5: replace prompt_bool -> Result<bool> with Ok(false) in 4s build + 6s test
MISSED   src/commands/init.rs:862:9: delete match arm "y" | "yes" in prompt_bool in 4s build + 5s test
MISSED   src/commands/init.rs:863:9: delete match arm "n" | "no" in prompt_bool in 4s build + 6s test
MISSED   src/commands/init.rs:869:5: replace prompt -> Result<String> with Ok(String::new()) in 4s build + 6s test
MISSED   src/commands/init.rs:869:5: replace prompt -> Result<String> with Ok("xyzzy".into()) in 4s build + 6s test
MISSED   src/commands/process.rs:73:17: replace && with || in run in 4s build + 5s test
MISSED   src/commands/process.rs:72:17: replace && with || in run in 6s build + 5s test
MISSED   src/commands/process.rs:73:20: delete ! in run in 5s build + 5s test
MISSED   src/commands/process.rs:139:24: delete ! in run in 4s build + 7s test
MISSED   src/commands/process.rs:263:13: delete match arm Op::RenameTopic(o) in run_teardown in 2s build + 5s test
MISSED   src/commands/process.rs:268:13: delete match arm Op::RenameAtomic(o) in run_teardown in 5s build + 5s test
MISSED   src/commands/process.rs:273:13: delete match arm Op::MoveAtomic(o) in run_teardown in 4s build + 8s test
MISSED   src/commands/process.rs:278:13: delete match arm Op::PromoteAtomic(o) in run_teardown in 5s build + 5s test
MISSED   src/commands/process.rs:283:13: delete match arm Op::DemoteTopic(o) in run_teardown in 4s build + 6s test
MISSED   src/commands/process.rs:288:13: delete match arm Op::MergeTopics(o) in run_teardown in 5s build + 6s test
MISSED   src/commands/process.rs:293:13: delete match arm Op::SplitTopic(o) in run_teardown in 5s build + 6s test
MISSED   src/commands/process.rs:298:13: delete match arm Op::SetEmbed(o) in run_teardown in 4s build + 7s test
MISSED   src/commands/process.rs:314:13: delete match arm Op::UpdateNote(op) in run_teardown in 3s build + 7s test
MISSED   src/commands/process.rs:404:17: replace > with == in run_teardown in 4s build + 6s test
MISSED   src/commands/process.rs:404:17: replace > with < in run_teardown in 4s build + 7s test
MISSED   src/commands/process.rs:404:17: replace > with >= in run_teardown in 4s build + 7s test
MISSED   src/commands/process.rs:411:16: replace > with == in run_teardown in 4s build + 5s test
MISSED   src/commands/process.rs:411:16: replace > with < in run_teardown in 4s build + 5s test
MISSED   src/commands/process.rs:411:16: replace > with >= in run_teardown in 5s build + 5s test
MISSED   src/commands/process.rs:435:5: replace collect_raw_note_roots -> Vec<std::path::PathBuf> with vec![] in 3s build + 6s test
MISSED   src/commands/process.rs:435:5: replace collect_raw_note_roots -> Vec<std::path::PathBuf> with vec![Default::default()] in 5s build + 7s test
MISSED   src/commands/process.rs:455:8: delete ! in resolve_scope in 4s build + 7s test
MISSED   src/commands/process.rs:528:13: delete match arm 0 in resolve_session_id in 4s build + 8s test
MISSED   src/commands/process.rs:540:9: delete match arm 0 in resolve_session_id in 4s build + 7s test
MISSED   src/commands/process.rs:953:9: delete match arm "p" in prompt_no_recording in 4s build + 7s test
MISSED   src/commands/process.rs:954:9: delete match arm "q" in prompt_no_recording in 5s build + 7s test
MISSED   src/commands/reconcile.rs:153:9: replace && with || in run_for_vault in 6s build + 9s test
MISSED   src/commands/reconcile.rs:152:9: replace && with || in run_for_vault in 4s build + 8s test
MISSED   src/commands/reconcile.rs:151:9: replace && with || in run_for_vault in 4s build + 9s test
MISSED   src/commands/reconcile.rs:150:9: replace && with || in run_for_vault in 5s build + 11s test
MISSED   src/commands/reconcile.rs:156:12: delete ! in run_for_vault in 5s build + 10s test
MISSED   src/commands/reconcile.rs:188:12: delete ! in run_for_vault in 5s build + 10s test
MISSED   src/commands/reconcile.rs:201:20: replace && with || in run_for_vault in 5s build + 17s test
MISSED   src/commands/reconcile.rs:201:23: delete ! in run_for_vault in 10s build + 12s test
MISSED   src/commands/reconcile.rs:359:58: replace == with != in scan_recordings_dir in 4s build + 11s test
MISSED   src/commands/reconcile.rs:378:5: replace build_date -> Option<NaiveDate> with Some(Default::default()) in 4s build + 9s test
MISSED   src/commands/reconcile.rs:393:5: replace handle_spaces -> Result<Option<PathBuf>> with Ok(None) in 4s build + 10s test
MISSED   src/commands/reconcile.rs:397:8: delete ! in handle_spaces in 6s build + 9s test
MISSED   src/commands/reconcile.rs:403:40: replace == with != in handle_spaces in 6s build + 12s test
MISSED   src/commands/reconcile.rs:418:9: delete match arm "transcript" in recording_kind in 14s build + 16s test
MISSED   src/commands/reconcile.rs:419:9: delete match arm "summary" in recording_kind in 5s build + 12s test
MISSED   src/commands/reconcile.rs:420:9: delete match arm "mindmap" in recording_kind in 5s build + 12s test
MISSED   src/commands/reconcile.rs:421:14: replace match guard q.len() == 1 && q.chars().next().is_some_and(|c| c.is_ascii_lowercase()) with true in recording_kind in 5s build + 11s test
MISSED   src/commands/reconcile.rs:421:14: replace match guard q.len() == 1 && q.chars().next().is_some_and(|c| c.is_ascii_lowercase()) with false in recording_kind in 9s build + 13s test
MISSED   src/commands/reconcile.rs:421:27: replace && with || in recording_kind in 4s build + 10s test
MISSED   src/commands/reconcile.rs:421:22: replace == with != in recording_kind in 5s build + 10s test
MISSED   src/commands/reconcile.rs:593:30: replace && with || in scan_sources_dir in 5s build + 10s test
MISSED   src/commands/reconcile.rs:593:26: replace > with >= in scan_sources_dir in 5s build + 10s test
MISSED   src/commands/reconcile.rs:634:13: delete match arm (_, Some(stem)) in scan_sources_dir in 4s build + 10s test
MISSED   src/commands/reconcile.rs:631:43: replace match guard !parent.is_empty() with true in scan_sources_dir in 5s build + 13s test
MISSED   src/commands/reconcile.rs:1354:5: replace notify with () in 7s build + 11s test
MISSED   src/commands/recover.rs:52:8: delete ! in run in 5s build + 13s test
MISSED   src/commands/recover.rs:142:9: delete match arm "r" | "resume" in prompt_choice in 4s build + 11s test
MISSED   src/commands/recover.rs:148:5: replace prompt_discard_only -> Result<()> with Ok(()) in 5s build + 16s test
MISSED   src/commands/recover.rs:153:9: delete match arm "y" | "Y" in prompt_discard_only in 5s build + 12s test
MISSED   src/commands/status.rs:11:5: replace run -> Result<()> with Ok(()) in 6s build + 17s test
MISSED   src/commands/status.rs:51:35: replace == with != in run in 6s build + 17s test
MISSED   src/commands/status.rs:56:30: replace == with != in run in 5s build + 15s test
MISSED   src/commands/status.rs:72:29: delete ! in run in 7s build + 16s test
MISSED   src/commands/status.rs:97:8: delete ! in run in 5s build + 16s test
MISSED   src/commands/status.rs:110:8: delete ! in run in 5s build + 18s test
MISSED   src/commands/status.rs:122:45: replace > with == in run in 5s build + 15s test
MISSED   src/commands/status.rs:122:45: replace > with < in run in 5s build + 13s test
MISSED   src/commands/status.rs:122:45: replace > with >= in run in 5s build + 15s test
MISSED   src/commands/status.rs:158:35: replace == with != in run in 5s build + 13s test
MISSED   src/commands/status.rs:163:20: replace > with == in run in 5s build + 11s test
MISSED   src/commands/status.rs:163:20: replace > with < in run in 6s build + 15s test
MISSED   src/commands/status.rs:163:20: replace > with >= in run in 5s build + 12s test
MISSED   src/commands/status.rs:179:24: replace > with == in run in 4s build + 11s test
MISSED   src/commands/status.rs:179:24: replace > with < in run in 10s build + 17s test
MISSED   src/commands/status.rs:179:24: replace > with >= in run in 5s build + 16s test
811 mutants tested in 86m: 256 missed, 505 caught, 48 unviable, 2 timeouts
