# GitCloner

Rust wrapper around git2/libgit2 for repository cloning and branch operations.

## What it does

Provides a higher-level interface over `git2` for cloning GitHub repositories with progress indication. Lists organisation repositories via `octocrab`, clones them locally, and checks out specified branches. Used as a shared dependency by other tools (e.g. Searcher).

## Usage

```rust
use git_cloner::github_authentication::authentication::GitHubCliAuthentication;
// Clone repos from a GitHub org with progress bars
```

Requires `gh` CLI installed and authenticated (via the `github_authentication` crate).

## Design Decisions

- **git2/libgit2 over shelling out to `git`**: Programmatic access to clone progress, error handling, and branch operations without parsing CLI output.
- **indicatif progress bars**: Visual feedback during potentially long clone operations.
- **Shared crate**: Extracted from Searcher to enable reuse across multiple CLI tools.
