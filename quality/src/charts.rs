//! Deterministic figures derived only from the report's validated measurements.
use crate::{
    benchmark,
    coverage::{Metrics, Summary},
    Result,
};
use plotters::{
    coord::Shift,
    prelude::*,
    style::text_anchor::{HPos, Pos, VPos},
};
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeSet, fmt::Write as _, fs, path::Path};

const BLUE: [u8; 3] = [0, 114, 178];
const GRAY: [u8; 3] = [148, 163, 184];
const COLORS: [[u8; 3]; 4] = [BLUE, [230, 159, 0], [0, 158, 115], [204, 121, 167]];
const LABELS: [&str; 4] = [
    "Capn't Proto / Native",
    "Cap'n Proto C++",
    "gRPC (tonic)",
    "WebSockets",
];
const FILES: [&str; 11] = [
    "latency-p50",
    "latency-p95",
    "latency-p99",
    "request-rate",
    "latency-difference",
    "request-rate-difference",
    "coverage-lines",
    "coverage-regions",
    "coverage-functions",
    "coverage-branches",
    "instruction-counts",
];
#[derive(Serialize)]
pub(crate) struct Bar {
    label: String,
    value: Option<f64>,
    color: [u8; 3],
}
#[derive(Serialize)]
pub(crate) struct Panel {
    title: String,
    bars: Vec<Bar>,
}
#[derive(Serialize)]
pub(crate) struct Chart {
    name: String,
    title: String,
    unit: String,
    note: String,
    coverage: bool,
    panels: Vec<Panel>,
}

/// Only remove files owned by this renderer, so a failed rerun cannot publish old figures.
pub(crate) fn clear(base: &Path) -> Result<()> {
    let directory = base.join("charts");
    for name in FILES
        .iter()
        .map(|name| format!("{name}.svg"))
        .chain(["data.json".into()])
    {
        match fs::remove_file(directory.join(name)) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub(crate) fn instructions(rows: &[crate::instructions::Row]) -> Chart {
    Chart {
        name: "instruction-counts".into(),
        title: "Serialization CPU work (Callgrind)".into(),
        unit: "Executed instructions".into(),
        note: "Maintained Rust serialization only; includes validation and ownership cleanup. Payload sizes perform different amounts of work. These are not RPC latency or cross-protocol comparisons; no historical regression baseline is established.".into(),
        coverage: false,
        panels: ["decode", "encode"].iter().map(|operation| Panel {
            title: (*operation).into(),
            bars: rows.iter().filter(|row| row.operation == *operation).map(|row| Bar {
                label: format!("{} bytes", row.payload_bytes),
                value: Some(row.instructions as f64), color: BLUE,
            }).collect(),
        }).collect(),
    }
}

pub(crate) fn performance(rows: &[Value]) -> Result<Vec<Chart>> {
    let mut charts = vec![];
    for (name, title, field, unit, divisor, relative) in [
        (
            "latency-p50",
            "Median round-trip latency (p50)",
            "p50_ns",
            "Microseconds - lower is better",
            1000.0,
            false,
        ),
        (
            "latency-p95",
            "Tail round-trip latency (p95)",
            "p95_ns",
            "Microseconds - lower is better",
            1000.0,
            false,
        ),
        (
            "latency-p99",
            "Tail round-trip latency (p99)",
            "p99_ns",
            "Microseconds - lower is better",
            1000.0,
            false,
        ),
        (
            "request-rate",
            "Sequential request rate",
            "sequential_requests_per_second",
            "Requests / second - higher is better",
            1.0,
            false,
        ),
        (
            "latency-difference",
            "Median latency difference from Capn't Proto",
            "p50_ns",
            "Percent difference - negative is less latency",
            1.0,
            true,
        ),
        (
            "request-rate-difference",
            "Request rate difference from Capn't Proto",
            "sequential_requests_per_second",
            "Percent difference - positive is more requests",
            1.0,
            true,
        ),
    ] {
        let mut panels = vec![];
        for bytes in benchmark::PAYLOADS {
            let cell = |protocol: &str| -> Result<f64> {
                let mut cells = rows
                    .iter()
                    .filter(|r| r["payload_bytes"] == bytes && r["protocol"] == protocol);
                let value = cells
                    .next()
                    .and_then(|r| r[field].as_f64())
                    .ok_or("missing chart measurement")?;
                if cells.next().is_some() || !value.is_finite() || value <= 0.0 {
                    return Err("invalid or duplicate chart measurement".into());
                }
                Ok(value)
            };
            let reference = cell("native")?;
            let mut bars = vec![];
            for (index, protocol) in benchmark::PROTOCOLS.iter().enumerate() {
                let value = cell(protocol)?;
                let value = if relative {
                    (value / reference - 1.0) * 100.0
                } else {
                    value / divisor
                };
                if !value.is_finite() {
                    return Err("chart conversion overflow".into());
                }
                bars.push(Bar {
                    label: LABELS[index].into(),
                    value: Some(value),
                    color: COLORS[index],
                });
            }
            panels.push(Panel {
                title: format!("{bytes} byte payload"),
                bars,
            });
        }
        charts.push(Chart { name: name.into(), title: title.into(), unit: unit.into(),
            note: "Each panel uses a linear axis including zero. Native is encrypted; other baselines are plaintext.".into(),
            coverage: false, panels });
    }
    Ok(charts)
}

pub(crate) fn coverage(current: &Summary, baseline: &Summary) -> Vec<Chart> {
    let groups: BTreeSet<_> = current
        .totals
        .keys()
        .chain(baseline.totals.keys())
        .collect();
    ["lines", "regions", "functions", "branches"].into_iter().map(|kind| {
        let metric = |m: &Metrics| match kind {
            "lines" => m.lines.percent(), "regions" => m.regions.percent(),
            "functions" => m.functions.percent(), _ => m.branches.percent(),
        };
        let mut bars = vec![];
        for group in &groups {
            let label = group.replace('-', " ");
            for (suffix, data, color) in [("baseline", baseline, GRAY), ("current", current, BLUE)] {
                bars.push(Bar { label: format!("{label} / {suffix}"), value: data.totals.get(*group).and_then(metric), color });
            }
        }
        Chart { name: format!("coverage-{kind}"), title: format!("LLVM {kind}: current versus reviewed baseline"),
            unit: "Coverage (%) - higher is better".into(),
            note: "Gray: reviewed baseline. Blue: current run. N/A: no mapped counters; never treated as 0% or 100%.".into(),
            coverage: true, panels: vec![Panel { title: "Source groups (different groups cover different code)".into(), bars }] }
    }).collect()
}

fn label(value: f64, percentage: bool) -> String {
    if percentage {
        format!("{value:+.1}%")
    } else if value != 0.0 && value.abs() < 0.01 {
        format!("{value:.2e}")
    } else if value.abs() >= 1000.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

fn draw_panel(
    area: &DrawingArea<SVGBackend<'_>, Shift>,
    spec: &Chart,
    panel: &Panel,
) -> Result<()> {
    let values: Vec<_> = panel.bars.iter().filter_map(|bar| bar.value).collect();
    if values.iter().any(|v| !v.is_finite()) || panel.bars.is_empty() {
        return Err("invalid chart values".into());
    }
    let negative = values.iter().copied().fold(0.0, f64::min);
    let positive = values.iter().copied().fold(0.0, f64::max);
    let (low, high) = if spec.coverage {
        (0.0, 100.0)
    } else {
        (
            negative * 1.35,
            if positive > 0.0 {
                positive * 1.35
            } else {
                (negative.abs() * 0.2).max(1.0)
            },
        )
    };
    let count = panel.bars.len();
    let mut chart = ChartBuilder::on(area)
        .caption(&panel.title, ("sans-serif", 19))
        .margin(18)
        .x_label_area_size(50)
        .y_label_area_size(if spec.coverage { 410 } else { 175 })
        .build_cartesian_2d(low..high, (count as f64 - 0.5)..-0.5)?;
    let y_label = |y: &f64| {
        let index = y.round();
        if (*y - index).abs() > 0.01 || index < 0.0 {
            String::new()
        } else {
            panel
                .bars
                .get(index as usize)
                .map(|b| b.label.clone())
                .unwrap_or_default()
        }
    };
    chart
        .configure_mesh()
        .disable_y_mesh()
        .max_light_lines(0)
        .y_labels(count)
        .x_labels(5)
        .y_label_formatter(&y_label)
        .label_style(("sans-serif", 16))
        .x_desc(&spec.unit)
        .axis_desc_style(("sans-serif", 15))
        .light_line_style(RGBColor(241, 245, 249))
        .bold_line_style(RGBColor(226, 232, 240))
        .draw()?;
    chart.draw_series(std::iter::once(PathElement::new(
        vec![(0.0, -0.5), (0.0, count as f64 - 0.5)],
        RGBColor(100, 116, 139),
    )))?;
    for (index, bar) in panel.bars.iter().enumerate() {
        let value = bar.value.unwrap_or(0.0);
        let color = RGBColor(bar.color[0], bar.color[1], bar.color[2]);
        if bar.value.is_some() && value != 0.0 {
            chart.draw_series(std::iter::once(Rectangle::new(
                [(0.0, index as f64 - 0.28), (value, index as f64 + 0.28)],
                color.filled(),
            )))?;
        }
        let inside = spec.coverage && value > 75.0;
        let align_right = inside;
        let style = ("sans-serif", 16)
            .into_text_style(area)
            .color(if inside && bar.color == BLUE {
                &WHITE
            } else {
                &BLACK
            })
            .pos(Pos::new(
                if align_right { HPos::Right } else { HPos::Left },
                VPos::Center,
            ));
        let text = bar
            .value
            .map(|v| {
                if spec.coverage {
                    format!("{v:.1}%")
                } else {
                    label(v, spec.name.ends_with("difference"))
                }
            })
            .unwrap_or_else(|| "N/A".into());
        chart.draw_series(std::iter::once(
            EmptyElement::at((value, index as f64))
                + Text::new(text, (if align_right { -6 } else { 6 }, 0), style),
        ))?;
    }
    Ok(())
}

fn render(directory: &Path, spec: &Chart, source: &str) -> Result<()> {
    let path = directory.join(format!("{}.svg", spec.name));
    let height = if spec.coverage {
        (spec.panels[0].bars.len() as u32 * 34 + 230).max(380)
    } else {
        730
    };
    let root = SVGBackend::new(&path, (1280, height)).into_drawing_area();
    root.fill(&WHITE)?;
    let (header, rest) = root.split_vertically(80);
    header.draw(&Text::new(
        spec.title.as_str(),
        (24, 30),
        ("sans-serif", 24).into_font().style(FontStyle::Bold),
    ))?;
    header.draw(&Text::new(spec.note.as_str(), (24, 61), ("sans-serif", 14)))?;
    let (plots, footer) = rest.split_vertically(height - 112);
    if spec.panels.len() == 1 {
        draw_panel(&plots, spec, &spec.panels[0])?;
    } else {
        for (area, panel) in plots.split_evenly((2, 2)).iter().zip(&spec.panels) {
            draw_panel(area, spec, panel)?;
        }
    }
    let scope = if spec.coverage {
        "Baseline and current denominators may differ"
    } else {
        "Same workload and payload within each panel"
    };
    footer.draw(&Text::new(
        format!("Source: {source} | Data: charts/data.json | {scope}"),
        (24, 20),
        ("sans-serif", 11),
    ))?;
    root.present()?;
    Ok(())
}

pub(crate) fn append(
    readme: &mut String,
    base: &Path,
    source: &str,
    charts: &[Chart],
) -> Result<()> {
    readme.push_str("\n## Comparison bar charts\n\n");
    if charts.is_empty() {
        readme.push_str("No validated coverage or benchmark measurements are available for charts in this run.\n");
        return Ok(());
    }
    let directory = base.join("charts");
    fs::create_dir_all(&directory)?;
    fs::write(
        directory.join("data.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"source_id":source,"charts":charts}))?,
    )?;
    readme.push_str("Figures below use the validated report data above. [Exact plotted values](charts/data.json) and standalone SVGs are included in this artifact. Performance panels compare protocols at the same payload size; axes include zero and are scaled separately per panel. Percentage differences use `(comparison / Capn't Proto - 1) × 100`. The coverage figures compare the current run with its reviewed baseline; unmapped counters remain N/A.\n\n");
    for chart in charts {
        render(&directory, chart, source)?;
        writeln!(
            readme,
            "### {}\n\n![{}](charts/{}.svg)\n",
            chart.title, chart.title, chart.name
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coverage::{Metric, Metrics};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn rows() -> Vec<Value> {
        let mut rows = vec![];
        for bytes in benchmark::PAYLOADS {
            for (index, protocol) in benchmark::PROTOCOLS.iter().enumerate() {
                let latency =
                    [10_000.0, 20_000.0, 5_000.0, 15_000.0][index] * (1.0 + bytes as f64 / 1024.0);
                rows.push(json!({"protocol":protocol, "payload_bytes":bytes, "p50_ns":latency,
                    "p95_ns":latency * 1.5, "p99_ns":latency * 2.0, "sequential_requests_per_second":1e9 / latency}));
            }
        }
        rows
    }
    fn summary() -> Summary {
        Summary {
            files: BTreeMap::new(),
            totals: BTreeMap::from([
                (
                    "runtime".into(),
                    Metrics {
                        lines: Metric {
                            covered: 80,
                            count: 100,
                        },
                        regions: Metric {
                            covered: 40,
                            count: 50,
                        },
                        functions: Metric {
                            covered: 0,
                            count: 10,
                        },
                        branches: Metric::default(),
                    },
                ),
                (
                    "tests-and-verification".into(),
                    Metrics {
                        lines: Metric {
                            covered: 100,
                            count: 100,
                        },
                        regions: Metric {
                            covered: 100,
                            count: 100,
                        },
                        functions: Metric {
                            covered: 100,
                            count: 100,
                        },
                        branches: Metric {
                            covered: 1,
                            count: 10,
                        },
                    },
                ),
            ]),
            flags: "TEST FIXTURE".into(),
            compiler: "TEST FIXTURE".into(),
            baseline: "TEST FIXTURE".into(),
        }
    }
    #[test]
    fn charts_preserve_units_directions_and_missing_counters() {
        let rows = rows();
        let charts = performance(&rows).unwrap();
        assert_eq!(charts.len(), 6);
        assert_eq!(charts[0].panels[0].bars[0].value, Some(10.0)); // ns -> microseconds
        assert_eq!(charts[3].panels[0].bars[0].value, Some(100_000.0));
        assert_eq!(charts[4].panels[0].bars[0].value, Some(0.0));
        assert_eq!(charts[4].panels[0].bars[1].value, Some(100.0)); // twice the latency
        assert_eq!(charts[4].panels[0].bars[2].value, Some(-50.0));
        assert_eq!(charts[5].panels[0].bars[1].value, Some(-50.0)); // half the request rate
        assert_eq!(charts[5].panels[0].bars[2].value, Some(100.0));
        assert!(performance(&rows[..rows.len() - 1]).is_err());
        let mut duplicates = rows.clone();
        duplicates.push(rows[0].clone());
        assert!(performance(&duplicates).is_err());
        let mut invalid = rows;
        invalid[0]["p50_ns"] = json!(0);
        assert!(performance(&invalid).is_err());
        let summary = summary();
        let charts = coverage(&summary, &summary);
        assert_eq!(charts[2].panels[0].bars[0].value, Some(0.0));
        assert_eq!(charts[3].panels[0].bars[0].value, None);
    }
    #[test]
    fn renders_suite_and_retains_na_instead_of_zero() {
        let temp = tempfile::tempdir().unwrap();
        let mut charts = performance(&rows()).unwrap();
        let mut baseline = summary();
        for group in [
            "cpp-reference",
            "maintained-capnp",
            "transport",
            "tools-fuzz-examples-benchmarks",
        ] {
            baseline.totals.insert(
                group.into(),
                baseline.totals["tests-and-verification"].clone(),
            );
        }
        let mut current = baseline.clone();
        current.totals.get_mut("runtime").unwrap().lines.covered = 90;
        charts.extend(coverage(&current, &baseline));
        let counts: Vec<_> = ["decode", "encode"]
            .iter()
            .flat_map(|operation| {
                [64, 1024, 65536].map(|payload_bytes| crate::instructions::Row {
                    operation: (*operation).into(),
                    payload_bytes,
                    instructions: payload_bytes + 100,
                    data_reads: payload_bytes,
                    data_writes: 0,
                })
            })
            .collect();
        charts.push(instructions(&counts));
        let mut readme = String::from("# Synthetic renderer test data - not benchmark results\n");
        append(
            &mut readme,
            temp.path(),
            "TEST FIXTURE - NOT A PERFORMANCE MEASUREMENT",
            &charts,
        )
        .unwrap();
        for name in FILES {
            let svg = fs::read_to_string(temp.path().join(format!("charts/{name}.svg"))).unwrap();
            assert!(svg.contains("<svg"));
            assert!(readme.contains(&format!("(charts/{name}.svg)")));
        }
        let branches =
            fs::read_to_string(temp.path().join("charts/coverage-branches.svg")).unwrap();
        assert!(branches.contains("N/A"));
        let functions =
            fs::read_to_string(temp.path().join("charts/coverage-functions.svg")).unwrap();
        assert!(functions.contains("0.0%"));
        for group in current.totals.keys() {
            for revision in ["baseline", "current"] {
                assert!(
                    functions.contains(&format!("{} / {revision}", group.replace('-', " "))),
                    "missing chart label: {group} / {revision}"
                );
            }
        }
        let first = fs::read(temp.path().join("charts/latency-difference.svg")).unwrap();
        let mut repeated = String::new();
        append(
            &mut repeated,
            temp.path(),
            "TEST FIXTURE - NOT A PERFORMANCE MEASUREMENT",
            &charts,
        )
        .unwrap();
        assert_eq!(
            first,
            fs::read(temp.path().join("charts/latency-difference.svg")).unwrap()
        );
        // Opt-in local visual inspection; these labelled fixtures are never CI evidence.
        if let Some(directory) = std::env::var_os("REPROTO_CHART_TEST_PREVIEW") {
            let directory = std::path::PathBuf::from(directory);
            fs::create_dir_all(directory.join("charts")).unwrap();
            for entry in fs::read_dir(temp.path().join("charts")).unwrap() {
                let entry = entry.unwrap();
                fs::copy(
                    entry.path(),
                    directory.join("charts").join(entry.file_name()),
                )
                .unwrap();
            }
            fs::write(directory.join("README.md"), readme).unwrap();
        }
    }
}
