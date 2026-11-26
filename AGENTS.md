# Agent instructions

## Branches

Day-to-day work goes on `dev`. Merging into `main` publishes a release that installed copies download automatically (see [README.md](README.md)).

## Pull requests into `main`

A PR into `main` is a release. Its description must contain a `## Release notes` section. When the PR is merged, `.github/workflows/release.yml` publishes that section, and only that section, as the GitHub Release description. Everything else in the PR description stays on the PR.

Write the release notes for users of the app, not for reviewers:

- Base them on what actually changed between `main` and `dev` (`git log main..dev` and the diff), not only on commit subjects.
- Describe what users will notice: new features, changed behaviour, fixed bugs. Leave out refactors, CI, docs, and other internal changes unless they affect users.
- Group the notes under `### New`, `### Improved`, and `### Fixed`, and skip any group that would be empty. Use short bullets in plain language.
- Don't use any `## ` heading inside the section, because the next `## ` heading ends it.
- Keep AI attribution and other PR boilerplate after the next `## ` heading or outside the section, so it never reaches the release.

Example:

```markdown
## Release notes

### New
- Drag achievements to reorder them on the shelf.

### Fixed
- The picker no longer crashes when a game has no achievements.

## Summary
...reviewer-facing details...
```

If the section is missing, the release falls back to a generic description, so always include it.
