# Wiki maintenance

Maintain the authoritative pages in `docs/wiki/` beside the code they describe.
Review them in ordinary pull requests. `scripts/wiki.py` creates a flat GitHub
Wiki export with `Home.md`, `_Sidebar.md` and `_Footer.md`; generated exports
belong under ignored `target/` and are not a second editable copy.

## Edit and validate

Use readable hyphenated page filenames, one subject per page, and ordinary
relative Markdown links ending in `.md`. Link source files relative to the page.
Commands run from the repository root unless explicitly stated otherwise.
Examples that depend on an application's schema/context are fragments; linked
Rust tests/examples provide executable forms.

```sh
python3 scripts/wiki.py check
python3 -m unittest discover -s scripts/tests -p 'test_wiki.py'
python3 scripts/wiki.py build --output target/wiki
```

The checker validates local destinations, Markdown heading anchors, navigation
and inventory coverage. It excludes upstream documentation, template-relative
links (checked in the generated README) and explicitly identified generated or
submodule artifacts absent from a fresh checkout. External URLs are checked by
the separate advisory Lychee CI lane. Passing local checks does not verify every
code fragment or certify external services.

The exporter converts links to maintained pages into GitHub wiki URLs, and links
to source/archive files into repository URLs at the selected source revision.
It records page hashes, repository identity and source ref in `wiki-export.json`.
No Git command, credential, commit or network write is performed by the exporter.
Existing unrelated output files cause an error instead of being deleted.

## Publish to GitHub

The configured source repository is
[DericHuynh/capntproto](https://github.com/DericHuynh/capntproto).
Its wiki lives in a separate Git repository, `capntproto.wiki.git`.
At this migration the source repository was empty and the wiki Git endpoint did
not exist, although the repository setting enabled wikis. These local pages are
ready for export; this document does not claim live publication.

1. Publish the reviewed source revision through the normal repository process.
   Source links in the wiki must name a branch or commit that exists on GitHub.
2. While signed in, open the repository's **Wiki** tab and create its first
   `Home` page. This initializes the wiki Git repository.
3. Export, clone the wiki and review the copied pages:

```sh
python3 scripts/wiki.py build --output target/wiki \
  --repository DericHuynh/capntproto --source-ref main
git clone https://github.com/DericHuynh/capntproto.wiki.git ../capntproto.wiki
cp target/wiki/*.md ../capntproto.wiki/
git -C ../capntproto.wiki add -- '*.md'
git -C ../capntproto.wiki diff --cached
```

4. Commit and push the reviewed wiki changes using the credentials already
   configured for Git. Do not force-push. If the wiki has independent edits,
   reconcile them into `docs/wiki/` first; copying intentionally replaces pages
   with the same names. Inspect obsolete pages explicitly rather than deleting
   an entire wiki checkout. GitHub displays the wiki's default branch.
5. Open Home and follow the sidebar, a cross-page anchor and a source-code link
   to verify the published rendering. For a release, use its published commit or
   tag as `--source-ref` instead of a moving branch.

[GitHub's page-editing guide](https://docs.github.com/en/communities/documenting-your-project-with-wikis/adding-or-editing-wiki-pages)
explains initialization and Git access; its
[sidebar/footer guide](https://docs.github.com/en/communities/documenting-your-project-with-wikis/creating-a-footer-or-sidebar-for-your-wiki)
defines the special filenames. The existing documentation workflow uploads a
validated export artifact. It does not publish or require a new access token.

## Keep one current account

- Put current usage/contracts here. Keep GitHub community files and package
  landing READMEs in their recognized locations, linking to the relevant guide.
- Keep old development ledgers in [the archive](../archive/README.md). Their
  counts, commands and former transports are historical, not current support.
- Keep frozen measurements and original source hashes under
  [research/reports](../../research/reports/README.md). Do not silently rerun into them.
- Edit `docs/README.template.md` for the repository README, then regenerate it
  with `python3 scripts/update_readme.py render`; see [README reports](README-Reports.md).
- Update `_Sidebar.md`, Home and `docs/documentation-review.json` when adding,
  merging or retiring pages. [Documentation review](Documentation-Review.md)
  records this migration's file-by-file decisions.
