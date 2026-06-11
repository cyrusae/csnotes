# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

### Security
- AppleScript argv injection fix (L3)
- safe_path validation for AI-produced paths + regression tests (L1)

### Added
- init: create per-course content dirs when active_courses is non-empty (L17)
- init: detect existing course folders and offer to populate active_courses (L16)
- config option to disable recordings expectation (global + per-course) (L12)
- cross_embedded_in rebuild in merge_back + dry_run scope output (L6)
- Phase 4: move_atomic, promote_atomic, demote_topic, merge_topics, split_topic, set_embed (L5)
- Agy backend: full GEMINI.md, model selection, report_schema clarification (L4)
- Phase 2: artifact detection, auto-reconcile, cross-platform notifications (L2)
- Phase 1: topics_updated, rename_topic, richer briefing, source pipeline (L1)

### Fixed

### Changed
- generalize plaud to recordings throughout codebase (L11)
- rename .csnotes config file to csnotes.toml (L10)
- Track: advisory locking on manifest (backlog) (L5)
- Proptest for markdown parsers (wikilinks, embeds, block IDs) (L4)
- CRLF normalization on file read + unit tests (L2)
- Phase 3: proptest idempotency, transactionality, and crash-recovery suites (L3)
