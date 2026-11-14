//! Measurement plans and the summary of repeated runs.
use super::*;

const CAPACITIES: [usize; 6] = [8, 16, 64, 256, 1024, 0];
const WORKERS: [usize; 5] = [1, 2, 4, 8, 16];

/// A collector run of `total` directories split evenly over `lanes` roots.
fn base(kind: Kind, total: u64, files: u32, lanes: usize) -> Config {
    Config {
        kind,
        dirs: total / lanes as u64,
        files,
        batch: 256,
        workers: 4,
        limit: 4,
        capacity: QUEUED_DIRECTORIES,
        lanes,
        delay_us: 0,
        mode: Mode::Collector,
    }
}

/// Every worker count and capacity of `seed`, with one worker limit per lane
/// equal to the worker count.
fn sweep(seed: Config, workers: &[usize], out: &mut Vec<Config>) {
    for &count in workers {
        for capacity in CAPACITIES {
            out.push(Config {
                workers: count,
                limit: count,
                capacity,
                ..seed
            });
        }
    }
}

/// Collector runs over every worker count; full runs over smaller trees,
/// because the result index time grows quickly with depth and size.
fn main(out: &mut Vec<Config>) {
    let trees = [
        (Kind::Wide, 50_000, 50_000, 4),
        (Kind::Deep, 4_000, 1_000, 4),
        (Kind::Mixed, 100_000, 20_000, 0),
    ];
    for (kind, collector, full, files) in trees {
        for lanes in [1, 4] {
            sweep(base(kind, collector, files, lanes), &WORKERS, out);
            let seed = Config {
                mode: Mode::Full,
                ..base(kind, full, files, lanes)
            };
            sweep(seed, &[1, 4, 16], out);
        }
    }
}

fn delay(out: &mut Vec<Config>) {
    for delay_us in [0, 50, 1000] {
        for lanes in [1, 4] {
            let seed = Config {
                delay_us,
                ..base(Kind::Mixed, 3_000, 0, lanes)
            };
            sweep(seed, &[1, 4, 16], out);
        }
    }
}

/// Full read batches held by queued and stacked frames, one root per lane.
fn batch(out: &mut Vec<Config>) {
    for (batch, dirs) in [(256, 2_000), (8192, 200)] {
        for lanes in [1, 4, 16] {
            let seed = Config {
                dirs,
                batch,
                files: batch as u32,
                ..base(Kind::Full, 0, 0, lanes)
            };
            for workers in [4, 16] {
                for capacity in [8, QUEUED_DIRECTORIES, 0] {
                    out.push(Config {
                        workers,
                        limit: workers,
                        capacity,
                        ..seed
                    });
                }
            }
            if lanes == 1 {
                out.push(Config {
                    mode: Mode::Full,
                    ..seed
                });
            }
        }
    }
}

fn large(out: &mut Vec<Config>) {
    out.push(Config {
        mode: Mode::Idle,
        ..base(Kind::Mixed, 0, 0, 1)
    });
    // A full run of one million directories needs tens of GB for the index.
    for (mode, total, capacities) in [
        (
            Mode::Collector,
            1_000_000,
            &[8, QUEUED_DIRECTORIES, 1024, 0][..],
        ),
        (Mode::Full, 100_000, &[8, QUEUED_DIRECTORIES, 0][..]),
    ] {
        for workers in [4, 16] {
            for &capacity in capacities {
                out.push(Config {
                    mode,
                    workers,
                    limit: workers,
                    capacity,
                    ..base(Kind::Mixed, total, 0, 1)
                });
            }
        }
    }
}

fn smoke(out: &mut Vec<Config>) {
    out.push(Config {
        mode: Mode::Idle,
        ..base(Kind::Mixed, 0, 0, 1)
    });
    for (kind, total, files) in [
        (Kind::Wide, 2_000, 4),
        (Kind::Deep, 400, 4),
        (Kind::Mixed, 2_000, 0),
    ] {
        for mode in [Mode::Collector, Mode::Full] {
            out.push(Config {
                mode,
                ..base(kind, total, files, 4)
            });
        }
    }
    out.push(Config {
        delay_us: 50,
        ..base(Kind::Mixed, 500, 0, 1)
    });
    out.push(Config {
        dirs: 20,
        batch: 8192,
        files: 8192,
        ..base(Kind::Full, 0, 0, 1)
    });
}

/// Configurations of a named plan: smoke, main, delay, batch or large.
pub(super) fn configs(plan: &str) -> Result<Vec<Config>, TestError> {
    let mut out = Vec::new();
    match plan {
        "smoke" => smoke(&mut out),
        "main" => main(&mut out),
        "delay" => delay(&mut out),
        "batch" => batch(&mut out),
        "large" => large(&mut out),
        other => return Err(format!("unknown plan {other}").into()),
    }
    Ok(out)
}

pub(super) fn line(fields: &BTreeMap<String, String>) -> String {
    let fields: Vec<String> = fields
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    fields.join(" ")
}

/// Median of every numeric field over the runs; other fields from the first.
pub(super) fn median(runs: &[BTreeMap<String, String>]) -> String {
    let Some(first) = runs.first() else {
        return String::new();
    };
    let mut summary = BTreeMap::new();
    for (key, value) in first {
        let mut values: Vec<f64> = runs
            .iter()
            .filter_map(|run| run.get(key)?.parse().ok())
            .collect();
        values.sort_by(f64::total_cmp);
        let text = match values.get(values.len() / 2) {
            Some(middle) if values.len() == runs.len() && value.contains('.') => {
                format!("{middle:.3}")
            }
            Some(middle) if values.len() == runs.len() => format!("{middle:.0}"),
            _ => value.clone(),
        };
        summary.insert(key.clone(), text);
    }
    line(&summary)
}
