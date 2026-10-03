"""Generate the root README and accessible SVG plots from bounded report history."""
from datetime import datetime, timezone
import html
import json
from pathlib import Path
import re

from .data import validate_publication

NAME = "Capn't Proto"
COLORS = ['#0868d4', '#00ac7b', '#ef4444', '#e89200', '#8b5cf6']
BENCHMARKS = ['latency-p50', 'latency-p95', 'latency-p99', 'request-rate',
              'latency-difference', 'request-rate-difference']


def empty_history():
    return {'format': 1, 'full': [], 'benchmark': None}


def validate_record(record):
    if set(record) != {'run_id', 'attempt', 'commit', 'date', 'url', 'conclusion', 'data'}:
        raise ValueError('unexpected history fields')
    if type(record['run_id']) is not int or record['run_id'] < 1 or type(record['attempt']) is not int or record['attempt'] < 1:
        raise ValueError('invalid run identity')
    if not re.fullmatch(r'[0-9a-f]{40}', record['commit']):
        raise ValueError('invalid source commit')
    date = datetime.fromisoformat(record['date'].replace('Z', '+00:00'))
    if date.tzinfo is None:
        raise ValueError('run date must include timezone')
    if not re.fullmatch(r'https://github\.com/[\w.-]+/[\w.-]+/actions/runs/\d+', record['url']):
        raise ValueError('invalid run URL')
    if record['conclusion'] not in ('success', 'failure', 'cancelled', 'timed_out', 'action_required', 'skipped', 'neutral', 'stale', 'startup_failure'):
        raise ValueError('invalid workflow conclusion')
    validate_publication(record['data'])
    return record


def order(record):
    return datetime.fromisoformat(record['date'].replace('Z', '+00:00')).astimezone(timezone.utc), record['run_id'], record['attempt']


def validate_history(history):
    if set(history) != {'format', 'full', 'benchmark'} or history['format'] != 1:
        raise ValueError('invalid history format')
    if not isinstance(history['full'], list) or len(history['full']) > 365:
        raise ValueError('invalid history size')
    ids = set()
    for record in history['full'] + ([history['benchmark']] if history['benchmark'] else []):
        validate_record(record)
        if record['run_id'] in ids:
            raise ValueError('duplicate history run')
        ids.add(record['run_id'])
    if any(r['data']['kind'] != 'full' for r in history['full']):
        raise ValueError('invalid full history lane')
    if history['benchmark'] and history['benchmark']['data']['kind'] != 'benchmark':
        raise ValueError('invalid benchmark history lane')
    if history['full'] != sorted(history['full'], key=order):
        raise ValueError('history is not chronological')
    return history


def merge(history, record):
    validate_history(history)
    validate_record(record)
    history = json.loads(json.dumps(history))
    if record['data']['kind'] == 'full':
        previous = next((r for r in history['full'] if r['run_id'] == record['run_id']), None)
        if previous and previous['attempt'] >= record['attempt']:
            return history
        history['full'] = sorted([r for r in history['full'] if r['run_id'] != record['run_id']] + [record], key=order)[-365:]
    elif not history['benchmark'] or order(record) > order(history['benchmark']):
        history['benchmark'] = record
    for old in history['full'][:-1]:
        old['data']['charts'] = []  # Keep historical test counts; only latest coverage needs bars.
        if old['data']['tests']:
            old['data']['tests'].pop('failures', None)
    return validate_history(history)


def failure_report(record):
    """Render validated names as escaped text, never executable Markdown/HTML."""
    text = '# Failed workspace tests\n\n'
    if record is None:
        return text + 'No full-quality run has been recorded.\n'
    validate_record(record)
    text += f"[Workflow run and full logs]({record['url']}) · commit `{record['commit']}` · attempt {record['attempt']}\n\n"
    return text + failure_details(record['data'])


def failure_details(data):
    validate_publication(data)
    text = ''
    tests = data['tests']
    if tests is None:
        return text + 'No test inventory was produced. A build or infrastructure failure is not a passing test run. Inspect the workflow logs.\n'
    text += f"Recorded: **{tests['failed']} failed**, **{tests['errors']} without a terminal result**, {tests['passed']} passed and {tests['skipped']} skipped.\n\n"
    if 'failures' not in tests:
        return text + 'This older report contains counts only. Failed test names and output are in the workflow artifacts.\n'
    if not tests['failures']:
        text += 'No completed test reported a failure.\n\n'
    for i, failure in enumerate(tests['failures'], 1):
        text += f"<details open>\n<summary>{i}. <code>{html.escape(failure['name'])}</code></summary>\n\n"
        text += f"<p>{html.escape(failure['suite'])}</p>\n<pre>{html.escape(failure['output'])}</pre>\n"
        if failure['truncated']:
            text += '\nDiagnostic excerpt truncated; full output is in the workflow artifact.\n'
        text += '\n</details>\n\n'
    if not tests['command_passed']:
        text += 'The workspace command failed or was incomplete. Build/infrastructure errors and tests that never finished are not invented as named failures.\n'
    return text


def plotting():
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt
    plt.rcParams.update({'font.family': 'DejaVu Sans', 'font.size': 11, 'svg.fonttype': 'none',
                         'svg.hashsalt': 'capnt-proto', 'axes.edgecolor': '#cbd5e1',
                         'axes.labelcolor': '#334155', 'text.color': '#1e293b',
                         'xtick.color': '#64748b', 'ytick.color': '#334155'})
    return plt


def save(plt, fig, path):
    fig.savefig(path, format='svg', facecolor='white', metadata={'Date': None, 'Creator': "Capn't Proto CI reporting"})
    plt.close(fig)


def test_chart(history, path):
    plt = plotting()
    fig, ax = plt.subplots(figsize=(15, 6.8))
    fig.subplots_adjust(left=.075, right=.975, bottom=.15, top=.74)
    fig.text(.075, .94, f'{NAME} — Workspace Test Results', fontsize=25, weight='bold')
    fig.text(.075, .885, 'Outer workspace tests + doctests · counts per CI run · UTC', fontsize=12, color='#64748b')
    ax.set_facecolor('#f8fafc')
    ax.set_ylabel('Number of tests', weight='bold')
    ax.set_xlabel('Run date (UTC)', weight='bold')
    ax.grid(axis='both', color='#e2e8f0', linewidth=.7)
    ax.set_axisbelow(True)
    series = [('total', 'Total'), ('passed', 'Pass'), ('failed', 'Fail'), ('errors', 'Error'), ('skipped', 'Skip')]
    measured = [r for r in history['full'] if r['data']['tests'] is not None]
    for (key, label), color in zip(series, COLORS):
        dates = [datetime.fromisoformat(r['date'].replace('Z', '+00:00')) for r in measured]
        values = [r['data']['tests'][key] for r in measured]
        ax.plot(dates, values, label=label, color=color, linewidth=2.3, marker='o' if len(dates) < 20 else None, markersize=4)
        if len(dates) > 1:
            ax.fill_between(dates, values, color=color, alpha=.07)
    ax.legend(ncol=5, loc='lower left', bbox_to_anchor=(0, 1.015), frameon=False)
    ax.set_ylim(bottom=0)
    latest = history['full'][-1] if history['full'] else None
    tests = latest['data']['tests'] if latest else None
    if tests is not None and tests['total']:
        lines = ['Latest recorded counts'] + [f"{label}: {tests[key]:,} ({tests[key] / tests['total']:.1%})" for key, label in series[1:]]
        lines.append(f"Total: {tests['total']:,}")
        fig.text(.97, .97, '\n'.join(lines), va='top', ha='right', fontsize=11,
                 bbox={'boxstyle': 'round,pad=.6', 'facecolor': 'white', 'edgecolor': '#cbd5e1'})
    if measured:
        import matplotlib.dates as mdates
        locator = mdates.AutoDateLocator(minticks=3, maxticks=8)
        ax.xaxis.set_major_locator(locator)
        ax.xaxis.set_major_formatter(mdates.ConciseDateFormatter(locator, tz=timezone.utc))
        for record in measured:
            if not record['data']['tests']['complete']:
                ax.axvline(datetime.fromisoformat(record['date'].replace('Z', '+00:00')), color='#ef4444', linestyle=':', alpha=.4)
        note = 'Dotted lines: failed/incomplete workspace command. Error = announced tests without a terminal result.'
    else:
        ax.set_xticks([])
        ax.set_yticks([])
        ax.text(.5, .5, 'Awaiting the first full-workspace CI measurement', transform=ax.transAxes,
                ha='center', fontsize=17, color='#64748b')
        note = 'No invented history. Missing measurements are not zero tests or passing tests.'
    if latest and tests is None:
        note = 'LATEST RUN HAS NO TEST COUNTS — see its CI log. Earlier points are historical evidence.'
    fig.text(.075, .035, note, fontsize=10, color='#64748b')
    save(plt, fig, path)


def bar_chart(chart, path, stamp):
    plt = plotting()
    panels = chart['panels']
    columns = 2 if len(panels) > 1 else 1
    rows = (len(panels) + columns - 1) // columns
    height = max(4.8, 2.1 + max(len(p['bars']) for p in panels) * .32) if columns == 1 else 8.5
    fig, axes = plt.subplots(rows, columns, figsize=(15, height), squeeze=False)
    fig.subplots_adjust(left=.21 if columns == 1 else .17, right=.96, top=.83, bottom=.16, hspace=.7, wspace=.8)
    fig.text(.035, .96, chart['title'].replace('ReProto', NAME), fontsize=22, weight='bold', va='top')
    fig.text(.035, .89, chart['note'].replace('ReProto', NAME), fontsize=10, color='#64748b', va='top', wrap=True)
    for ax, panel in zip(axes.flat, panels):
        labels = [b['label'].replace('ReProto', NAME) for b in panel['bars']]
        values = [b['value'] for b in panel['bars']]
        numeric = [v for v in values if v is not None]
        colors = ['#0868d4', '#e69f00', '#009e73', '#cc79a7']
        ax.set_facecolor('#f8fafc')
        ax.set_title(panel['title'], fontsize=13, weight='bold', loc='left', pad=14)
        ax.set_yticks(range(len(labels)), labels)
        ax.invert_yaxis()
        ax.set_xlabel(chart['unit'], fontsize=10)
        ax.grid(axis='x', color='#e2e8f0')
        ax.set_axisbelow(True)
        for i, v in enumerate(values):
            if v is None:
                ax.text(.01, i, 'N/A', transform=ax.get_yaxis_transform(), va='center', fontsize=10)
                continue
            ax.barh(i, v, height=.55, color=colors[i % len(colors)], zorder=3)
            label = f'{v:+.1f}%' if chart['name'].endswith('difference') else f'{v:,.2f}' if abs(v) < 1000 else f'{v:,.0f}'
            ax.annotate(label, (v, i), xytext=(-5 if v < 0 else 5, 0), textcoords='offset points',
                        ha='right' if v < 0 else 'left', va='center', fontsize=10)
        low, high = min([0] + numeric), max([0] + numeric)
        margin = max(high - low, 1) * .26
        ax.set_xlim(low - margin if low < 0 else 0, high + margin)
        ax.axvline(0, color='#94a3b8', linewidth=.8)
    for ax in list(axes.flat)[len(panels):]:
        ax.set_visible(False)
    fig.text(.035, .055, stamp, fontsize=10, color='#64748b')
    save(plt, fig, path)


def waiting_chart(path):
    plt = plotting()
    fig, ax = plt.subplots(figsize=(15, 3))
    ax.axis('off')
    ax.text(.03, .78, f'{NAME} — Linux Loopback Benchmarks', fontsize=23, weight='bold', transform=ax.transAxes)
    ax.text(.03, .43, 'Awaiting a validated dedicated-host benchmark run', fontsize=16, color='#64748b', transform=ax.transAxes)
    ax.text(.03, .16, "Capn't Proto / Native · C++ Cap'n Proto · gRPC (tonic) · WebSockets\nLabelled latency, request-rate and relative-difference bars appear after measurements arrive.", fontsize=11, transform=ax.transAxes)
    save(plt, fig, path)


def run_text(record):
    return f"[{record['date']} · run {record['run_id']} / attempt {record['attempt']}]({record['url']}) · commit `{record['commit'][:12]}` · **{record['conclusion']}**"


def render(template, history, output):
    validate_history(history)
    if template.count('{{REPORTS}}') != 1:
        raise ValueError('README template must contain exactly one {{REPORTS}} slot')
    output = Path(output)
    assets = output / 'docs/reports'
    assets.mkdir(parents=True, exist_ok=True)
    # Remove only renderer-owned figures. Missing/failed new data cannot leave
    # an old bar chart looking like the newest measurement.
    from .data import CHARTS
    for name in CHARTS:
        (assets / f'{name}.svg').unlink(missing_ok=True)
    (assets / 'benchmarks-pending.svg').unlink(missing_ok=True)
    test_chart(history, assets / 'test-history.svg')
    section = '## Tests, coverage and benchmarks\n\n'
    section += 'Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.\n\n'
    full = history['full'][-1] if history['full'] else None
    benchmark = history['benchmark']
    section += '### Aggregate workspace tests\n\n'
    section += ('Latest full-quality run: ' + run_text(full) + '\n\n') if full else 'Awaiting the first full-quality CI run. No historical results have been fabricated.\n\n'
    section += '![Aggregate test history: total, passed, failed, errored and skipped](docs/reports/test-history.svg)\n\n'
    if full and full['data']['tests']:
        t = full['data']['tests']
        section += '| Total | Passed | Failed | Errors | Skipped | Workspace command |\n| ---: | ---: | ---: | ---: | ---: | --- |\n'
        section += f"| {t['total']:,} | {t['passed']:,} | [{t['failed']:,}](docs/reports/failed-tests.md) | {t['errors']:,} | {t['skipped']:,} | {'Passed' if t['command_passed'] else 'Failed / incomplete'} |\n\n"
    section += '[Show all failed tests and diagnostics](docs/reports/failed-tests.md).\n\n'
    section += 'Counts are outer workspace libtest cases and doctests. Nested C++/model/fuzz checks are represented by their parent test, without double-counting their internal cases. Skipped means ignored; errors mean announced tests that never returned a result. Build failures and missing reports have unknown totals. [Reporting contract and setup](docs/wiki/README-Reports.md).\n\n'
    section += '### LLVM coverage\n\n'
    coverage = full['data']['charts'] if full else []
    for chart in coverage:
        bar_chart(chart, assets / f"{chart['name']}.svg", f"Measured {full['date']} | commit {full['commit'][:12]} | reviewed baseline comparison")
        section += f"![{chart['title']}](docs/reports/{chart['name']}.svg)\n\n"
    if not coverage:
        section += 'No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.\n\n'
    section += '### Linux loopback benchmark comparisons\n\n'
    section += ('Latest benchmark run: ' + run_text(benchmark) + '\n\n') if benchmark else 'Awaiting the first dedicated DigitalOcean benchmark run.\n\n'
    section += "Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Capn't Proto uses encrypted Native/UDP; C++ Cap'n Proto, gRPC and WebSocket baselines use plaintext TCP. Bars compare this workload, not universal protocol performance.\n\n"
    charts = benchmark['data']['charts'] if benchmark else []
    for chart in charts:
        bar_chart(chart, assets / f"{chart['name']}.svg", f"Measured {benchmark['date']} | commit {benchmark['commit'][:12]} | dedicated Linux loopback")
        section += f"![{chart['title'].replace('ReProto', NAME)}](docs/reports/{chart['name']}.svg)\n\n"
    if not charts:
        waiting_chart(assets / 'benchmarks-pending.svg')
        section += '![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)\n\n'
    section += '[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.\n'
    (output / 'README.md').write_text(template.replace('{{REPORTS}}', section))
    (assets / 'failed-tests.md').write_text(failure_report(full))
    (assets / 'history.json').write_text(json.dumps(history, indent=2) + '\n')
    return [output / 'README.md', assets / 'history.json', assets / 'failed-tests.md', *sorted(assets.glob('*.svg'))]
