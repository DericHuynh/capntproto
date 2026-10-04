"""Contract tests for wiki migration links and non-destructive exports."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('wiki', Path(__file__).resolve().parents[1] / 'wiki.py')
wiki = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(wiki)


class WikiTests(unittest.TestCase):
    def setUp(self):
        self.work = tempfile.TemporaryDirectory()
        self.addCleanup(self.work.cleanup)
        self.root = Path(self.work.name)
        self.pages = self.root / 'docs/wiki'
        self.pages.mkdir(parents=True)
        (self.pages / 'Home.md').write_text('# Home\n\n[Guide](Guide.md#hello-world)\n[Source](../../src/lib.rs)\n')
        (self.pages / 'Guide.md').write_text('# Guide\n\n## Hello `world`!\n\n[Home](Home.md)\n')
        (self.pages / '_Sidebar.md').write_text('[Home](Home.md)\n[Guide](Guide.md)\n')
        (self.pages / '_Footer.md').write_text('[Home](Home.md)\n')
        (self.root / 'src').mkdir()
        (self.root / 'src/lib.rs').write_text('// source')
        self.output = self.root / 'target/wiki'

    def build(self):
        return wiki.build(self.root, self.output, 'owner/repository', 'release/next')

    def test_export_links_and_repeat_are_deterministic(self):
        self.assertEqual(wiki.check(self.root), [])
        self.assertEqual(self.build(), 4)
        before = {p.name: p.read_bytes() for p in self.output.iterdir()}
        home = (self.output / 'Home.md').read_text()
        self.assertIn('https://github.com/owner/repository/wiki/Guide#hello-world', home)
        self.assertIn('https://github.com/owner/repository/blob/release%2Fnext/src/lib.rs', home)
        self.build()
        self.assertEqual(before, {p.name: p.read_bytes() for p in self.output.iterdir()})
        self.assertIn('Guide.md#hello-world', (self.pages / 'Home.md').read_text())

    def test_source_directory_external_and_local_anchors(self):
        source = self.pages / 'Guide.md'
        for old, expected in [('../../src/', 'https://github.com/a/b/tree/main/src'),
                              ('#guide', '#guide'),
                              ('https://example.com/a(b)#x', 'https://example.com/a(b)#x')]:
            self.assertEqual(wiki.export_url(self.root, source, old, 'a/b', 'main'), expected)

    def test_broken_destinations_and_anchors_stop_export(self):
        (self.pages / 'Guide.md').write_text('# Guide\n[Missing](missing.md)\n[Bad](Home.md#missing)')
        errors = '\n'.join(wiki.check(self.root))
        self.assertIn('missing target missing.md', errors)
        self.assertIn('missing anchor Home.md#missing', errors)
        with self.assertRaises(ValueError):
            self.build()
        self.assertFalse(self.output.exists())

    def test_code_examples_are_not_followed_or_rewritten(self):
        text = ('```md\n[Fake](missing.md)\n```\n~~~\n[Fake](missing.md)\n~~~\n'
                '`[Fake](missing.md)`\n    [Fake](missing.md)\n<!-- [Fake](missing.md) -->\n'
                '[Real](Guide.md)\n![Image](../../image.svg)\n[ref]: <Home.md>\n')
        self.assertEqual([url for _, _, url in wiki.links(text)], ['Guide.md', '../../image.svg', 'Home.md'])
        rendered = wiki.rewrite_links(text, lambda url: 'changed/' + url)
        self.assertEqual(rendered.count('changed/'), 3)
        self.assertEqual(rendered.count('[Fake](missing.md)'), 5)

    def test_github_heading_anchors(self):
        text = '# Hello `world`!\n## Hello world\n## Hello world-1\nTitle\n=====\n'
        self.assertEqual(wiki.anchors(text), {'hello-world', 'hello-world-1', 'hello-world-1-1', 'title'})

    def test_unrelated_output_is_never_removed(self):
        self.output.mkdir(parents=True)
        extra = self.output / 'notes.md'
        extra.write_text('human work')
        with self.assertRaisesRegex(ValueError, 'nonempty output'):
            self.build()
        self.assertEqual(extra.read_text(), 'human work')

    def test_modified_export_is_never_overwritten(self):
        self.build()
        page = self.output / 'Guide.md'
        page.write_text('independent wiki edit')
        with self.assertRaisesRegex(ValueError, 'previous export changed'):
            self.build()
        self.assertEqual(page.read_text(), 'independent wiki edit')

    def test_extra_file_in_owned_export_is_preserved(self):
        self.build()
        extra = self.output / 'notes.txt'
        extra.write_text('notes')
        with self.assertRaisesRegex(ValueError, 'unrelated files'):
            self.build()
        self.assertEqual(extra.read_text(), 'notes')

    def test_remove_only_stale_owned_export_page(self):
        self.build()
        (self.pages / 'Guide.md').unlink()
        (self.pages / 'Home.md').write_text('# Home\n')
        (self.pages / '_Sidebar.md').write_text('[Home](Home.md)')
        self.assertEqual(self.build(), 3)
        self.assertFalse((self.output / 'Guide.md').exists())

    def test_refuse_source_directory_and_symlink(self):
        with self.assertRaisesRegex(ValueError, 'source documents'):
            wiki.build(self.root, self.pages, 'a/b', 'main')
        alias = self.root / 'alias'
        alias.symlink_to(self.pages, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'symlinks'):
            wiki.build(self.root, alias, 'a/b', 'main')

    def test_navigation_and_case_collisions(self):
        (self.pages / 'guide.md').write_text('# lowercase collision')
        errors = '\n'.join(wiki.check(self.root))
        self.assertIn('duplicate wiki page name', errors)
        self.assertIn('absent from sidebar', errors)
        self.assertNotIn('unregistered document', errors)

    def test_new_documents_are_checked_without_manual_inventory(self):
        (self.root / 'new.md').write_text('[broken](missing.md)')
        self.assertIn('missing target', '\n'.join(wiki.check(self.root)))

    def test_escape_and_generated_output_boundary(self):
        with self.assertRaisesRegex(ValueError, 'escapes repository'):
            wiki.local_target(self.root, self.pages / 'Home.md', '../../../../secret.md')
        home = self.pages / 'Home.md'
        home.write_text('# Home\n[Generated](../../target/report.json)\n[Missing](../../research/missing.json)')
        errors = '\n'.join(wiki.check(self.root))
        self.assertNotIn('target/report.json', errors)
        self.assertIn('research/missing.json', errors)


if __name__ == '__main__':
    unittest.main()
