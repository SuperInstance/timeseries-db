//! Write throughput benchmark

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use timeseries_db::{TimeSeriesDB, Point};
use tempfile::TempDir;
use std::time::Duration;

fn bench_write_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_throughput");

    for num_points in [1000, 10_000, 100_000, 1_000_000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(num_points),
            num_points,
            |b, &num_points| {
                b.iter(|| {
                    let temp_dir = TempDir::new().unwrap();
                    let mut db = TimeSeriesDB::open(temp_dir.path()).unwrap();

                    for i in 0..num_points {
                        db.write(Point {
                            timestamp: i,
                            metric: "bench.metric".into(),
                            value: i as f64,
                            tags: Default::default(),
                        }).unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_write_throughput);
criterion_main!(benches);
