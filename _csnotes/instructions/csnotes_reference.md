# csnotes — command reference

Quick reference for all `csnotes` CLI commands.  Run any command with `--help`
for the full flag list.

---

## Daily workflow

```sh
csnotes status                        # dashboard: pending sessions, topics, flags
csnotes status --json                 # compact JSON (for scripting or AI context)
csnotes status --topic <name>         # detailed view of one topic
csnotes reconcile                     # scan vault and register new files in manifest
csnotes process                       # launch AI session against the pending session
csnotes diff                          # semantic diff of what the last session changed
```

## Processing

```sh
csnotes process                       # auto-picks the one unprocessed session
csnotes process --next                # oldest pending (backlog catch-up)
csnotes process --session 09-03       # specific session by date
csnotes process --session 09-03 --for-course CPSC5001  # disambiguate same date in multiple courses
csnotes process --course CPSC5002     # course-wide review workspace (all sessions + sources)
csnotes process --course CPSC5002 --include-resource ml-refresher  # add resource notes to course workspace
csnotes process --source SICP/ch01   # process a source file
csnotes process --source Textbooks/SICP  # expand prefix → all sources under path
csnotes process --topic <name>        # focused review of an existing topic
csnotes process --resource Coursera/ML-Refresher  # process a self-study resource
csnotes process --resource ml-refresher  # fuzzy match (unique folder names)
csnotes process --backend agy         # override AI backend
csnotes process --dry-run             # show scope without launching
csnotes process --resume              # re-enter an interrupted session
```

## Mid-session (run from inside the workspace)

```sh
csnotes commit                        # execute current batch of ops, merge to vault
csnotes commit --dry-run              # preview ops and preconditions without executing
csnotes check                         # validate wikilinks, anchors, block IDs
csnotes report-schema                 # print the current _session_report.json schema
```

## Recovery

```sh
csnotes recover                       # show recovery options after a crash
csnotes recover --resume              # re-enter the interrupted session
csnotes recover --discard             # discard the workspace, leave vault untouched
csnotes recover --reset               # rebuild _synthetic/ from vault, clear report
```

## Audit and repair

```sh
csnotes audit                         # read-only invariant check across the vault
csnotes audit --reindex               # rebuild csnotes.json from frontmatter + filesystem
csnotes audit --fix                   # show mechanical repairs (dry-run)
csnotes audit --fix --apply           # execute repairs
csnotes audit --show-discrepancies    # diff manifest topics vs actual filesystem
```

## Reconcile

```sh
csnotes reconcile                     # scan and register new files
csnotes reconcile --notify            # desktop notification when new files found
csnotes reconcile --rename-spaces hyphens     # rename files with spaces
csnotes reconcile --rename-spaces underscores
csnotes reconcile --reset             # wipe and re-scan from scratch
```

## Configuration

```sh
csnotes config --show                 # print current config
csnotes config --set key=value        # set a config key
csnotes config --add-course CPSC5001  # add a course
csnotes config --archive CPSC5001     # remove from active courses
csnotes config --migrate              # show rename plan for current filename_format
csnotes config --migrate --apply      # execute the rename plan
```

## Flags

```sh
csnotes flags list                    # list open actionable flags
csnotes flags list --all              # include threads and changelog flags
csnotes flags show <id>               # full detail for one flag
csnotes flags resolve <id>            # mark a flag resolved
csnotes flags resolve <id> --follow-up "..."  # record a note at resolution
```

## Resources

Self-directed study notes (Coursera courses, tutorial videos, books) live in
`resources/` at the vault root.  Use type-named subdirectories so the kind is
inferred automatically:

```
resources/
  Coursera/
    ML-Refresher/       ← kind: OnlineCourse  (resource ID: Coursera/ML-Refresher)
      notes-week1.md
      notes-week2.md
  Tutorial/
    Rust-Async/         ← kind: Tutorial      (resource ID: Tutorial/Rust-Async)
      notes.md
  Book/
    CLRS/               ← kind: Book          (resource ID: Book/CLRS)
      ch01-notes.md
  anything-else/        ← kind: Other
```

Notes inside a resource folder can have any filename.  Add `date: YYYY-MM-DD`
in frontmatter to sort them chronologically in the workspace; undated notes
sort after dated ones by filename.

```sh
# After adding notes:
csnotes reconcile                                    # registers new resource folder
csnotes process --resource ml-refresher              # fuzzy match (unique folder names)
csnotes process --resource Coursera/ML-Refresher     # exact ID

# Include resource notes alongside a course workspace:
csnotes process --course CPSC5002 --include-resource ml-refresher
```

The `resources_dir` config key changes the folder name (default: `resources`).

## Setup

```sh
csnotes init                          # scaffold a new vault
csnotes init --instructions-only      # refresh instruction files without touching config
```

---

## Key files

| File | Purpose |
|---|---|
| `csnotes.toml` | Vault configuration (directories, courses, AI backend) |
| `csnotes.json` | Manifest: sessions, sources, topics, in-progress state |
| `_csnotes/instructions/` | AI instruction files (updated by `--instructions-only`) |
| `_generated/` | CLI-generated outputs: extracts, flags, session reports |
| `_synthetic/` | AI-maintained synthesis notes |
| `resources/` | Self-directed study notes (Coursera, tutorials, books) |
