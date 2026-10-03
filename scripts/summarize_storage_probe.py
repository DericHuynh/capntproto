#!/usr/bin/env python3
"""Summarize storage_probe runs without treating per-run p99s as pooled samples."""
import argparse
from collections import defaultdict
import json
from pathlib import Path
import statistics


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    groups = defaultdict(list)
    for line in (args.directory / 'runs.jsonl').read_text().splitlines():
        row = json.loads(line)
        key = (row['mode'], row.get('components', row.get('batch_size', 0)), row.get('held_snapshots', False))
        groups[key].append(row)
    summary = []
    for (mode, size, held), rows in sorted(groups.items()):
        if mode in ('whole', 'components'):
            fields = ['bytes_per_update', 'warm_acquire_and_hot_read_mean_us', 'warm_reopen_us',
                      'history_checkpoint_bytes', 'history_compact_us', 'commit_including_encode.p50_us',
                      'commit_including_encode.p99_us', 'mapped_before.mappings', 'mapped_before.virtual_bytes',
                      'mapped_after_compact.mappings', 'mapped_after_release.mappings']
        elif mode == 'batch':
            fields = ['mean_us_per_object_update', 'batch_commit.p50_us', 'batch_commit.p99_us', 'syncs']
        else:
            fields = ['raw_component_commit.p50_us', 'raw_component_commit.p99_us', 'timer_lateness.p99_us',
                      'timer_lateness.max_us', 'timer_lateness.count', 'elapsed_us']
        item = {'mode': mode, 'size': size, 'held_snapshots': held, 'trials': len(rows), 'metrics': {}}
        for field in fields:
            values = []
            for row in rows:
                value = row
                for key in field.split('.'):
                    value = value[key]
                values.append(value)
            item['metrics'][field] = {'median': statistics.median(values), 'min': min(values), 'max': max(values)}
        summary.append(item)
    (args.directory / 'summary.json').write_text(json.dumps({
        'aggregation': 'Median of per-run statistics; p99 values are not pooled; ranges show run-to-run variation.',
        'scenarios': summary,
    }, indent=2) + '\n')


if __name__ == '__main__':
    main()
