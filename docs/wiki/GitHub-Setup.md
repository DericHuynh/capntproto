# GitHub repository setup

The community files are ready to upload with the source. GitHub recognizes these
files in their supported locations; a repository description and private
vulnerability reporting are separate repository settings. No GitHub repository
settings are changed by adding these files.

## Source size and dependencies

Commit source and Cargo.lock files, never build output. Nested `target/`
directories are ignored, and `python3 scripts/check_repository.py` checks the
Git index for tracked ignored files and blobs over 50 MiB. GitHub
[blocks files larger than 100 MiB](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-large-files-on-github).
Adding `.gitignore` or deleting a file in a later commit does not remove blobs
from earlier commits; those blobs are still sent during the first push.

The C++ reference uses a pinned submodule; after cloning, run:

```sh
git submodule update --init --depth 1 -- vendor/capnproto
```

Cargo fetches ordinary Rust dependencies. The modified Rust forks remain local
path dependencies to preserve their coordinated APIs. See the
[dependency policy](Repository-Layout.md#vendored-and-external-code).

The initial local cleanup preserved the original commit under
`refs/backups/pre-github-cleanup` and replaced the unpublished initial commit.
Push the source branch explicitly with `git push -u origin main`; do not use
`git push --mirror`, which would include the local recovery reference. The
backup and reflog can keep the local `.git` directory large without making
the `main` branch upload large.

## Description and project links

Copy the single line from [DESCRIPTION.md](../../.github/DESCRIPTION.md) into the
repository's **About → Description** field:

> Capntproto: Rust schemas, serialization and capability RPC, with a native compiler, compatibility adapters, encrypted transport and correctness tooling.

GitHub does not automatically import `DESCRIPTION.md` into that field. The generated
[README](../../README.md) introduces the same project. Suggested topics
are `rust`, `rpc`, `capnproto`, `object-capabilities`, `quic`, `tla-plus`,
and `storage`.

The configured repository is **DericHuynh/capntproto**; the display name is
**Capntproto**. Project-owned Cargo packages/imports now use `capntproto`; see the
[name migration](Repository-Layout.md#crate-name-migration).
The [wiki publishing guide](Wiki-Maintenance.md) covers initialization, export
and the separate wiki Git repository.

The generated README and initial report SVGs are checked in. Follow
[README Reports](README-Reports.md) to enable automatic updates after Cargo, TLA+, fuzzing and
benchmark runs. Edit [the template](../README.template.md) for prose changes.

## Community files

| Purpose | File |
| --- | --- |
| Project overview and getting started | [README.md](../../README.md) |
| Participation and private conduct reports | [CODE_OF_CONDUCT.md](../../CODE_OF_CONDUCT.md) |
| Development and contribution guidelines | [CONTRIBUTING.md](../../CONTRIBUTING.md) |
| License for original project code | [LICENSE](../../LICENSE) |
| Vendored licenses and provenance | [THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md) |
| Private vulnerability reports and support scope | [SECURITY.md](../../SECURITY.md) |
| Usage questions and help | [SUPPORT.md](../../SUPPORT.md) |
| Bug, feature, documentation, and question templates | [ISSUE_TEMPLATE](../../.github/ISSUE_TEMPLATE) |
| Issue chooser and private contact links | [config.yml](../../.github/ISSUE_TEMPLATE/config.yml) |
| Pull request guidance | [PULL_REQUEST_TEMPLATE.md](../../.github/PULL_REQUEST_TEMPLATE.md) |

After uploading these files to the default branch, enable Issues if needed and
open **New issue** to verify the template chooser. Check **Insights → Community
Standards** for GitHub's community profile. Templates have no automatic labels
or assignees, so they do not depend on labels or teams that may not exist yet.

The maintainer contact for conduct and security reports is
[huynhderic@gmail.com](mailto:huynhderic@gmail.com).

## Enable private vulnerability reporting

For the public repository, open **Settings → Advanced Security**, find
**Private vulnerability reporting**, and enable it. GitHub places Advanced
Security under its **Security and quality** settings group.

Confirm that **Security → Advisories** offers **Report a vulnerability**. Enable
security-alert notifications for the maintainer so private reports are seen.
The email route in `SECURITY.md` also works before this setting is enabled.
GitHub's [private reporting setup guide](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/configure-vulnerability-reporting/configure-for-a-repository)
documents the setting and notification options.

## Code scanning

Use GitHub CodeQL **default setup** with the **Extended** security query suite
for Actions, C/C++, Python and Rust. Keep its branch/PR analysis and weekly scan
enabled. Check that the setup validation finishes successfully before treating
a configuration change as active. Do not also enable an advanced CodeQL workflow;
that would duplicate analysis. Include the vendored runtimes and review findings
using the [security policy](../../SECURITY.md) and
[recorded triage](../../.github/codeql-review.json). A dismissed alert must explain
the checked guard, public protocol value or test-only scope; do not exclude whole
queries or directories to remove alert counts.

GitHub documents [editing default setup](https://docs.github.com/en/code-security/how-tos/find-and-fix-code-vulnerabilities/manage-your-configuration/edit-default-setup)
and [resolving individual alerts](https://docs.github.com/en/code-security/how-tos/manage-security-alerts/manage-code-scanning-alerts/resolve-alerts).

## CI and repository policy

Follow [Quality and Benchmarks](Quality-and-Benchmarks.md) to configure the DigitalOcean GitHub Actions
secret, run the hosted workflows, and establish a reviewed LLVM coverage
baseline. Once the platform workflow has run, require **Required CI result** in the default branch's rules. Full verification and dedicated
benchmarks have separate workflows; their reports identify the checked revision.

## GitHub references

- [Community profile checklist](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/about-community-profiles-for-public-repositories)
- [Supported community health files](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/creating-a-default-community-health-file)
- [Issue template configuration](https://docs.github.com/en/communities/using-templates-to-encourage-useful-issues-and-pull-requests/configuring-issue-templates-for-your-repository)
