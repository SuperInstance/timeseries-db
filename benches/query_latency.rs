//! Query latency benchmark

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use timeseries_db::{TimeSeriesDB, Point, Query};
use tempfile::TempDir;
use std::time::Duration;

fn bench_query_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_latency");

    // Setup database with test data
    let temp_dir = TempDir::new().unwrap();
    let mut db = TimeSeriesDB::open(temp_dir.path()).unwrap();

    // Write 100K points
    for i in 0..100_000 {
        db.write(Point {
            timestamp: i,
            metric: "bench.metric".into(),
            value: i as f64,
            tags: Default::default(),
        }).unwrap();
    }

    // Benchmark query latency for different time ranges
    for range_hours in [1, 24, 168].iter() {  // 1h, 1d, 1w
        group.bench_with_input(
            BenchmarkId::new("range_hours", range_hours),
            range_hours,
            |b, &range_hours| {
                let start = 100_000 - range_hours * 3600;
                let end = 100_000;

                b.iter(|| {
                    let query = Query::new("bench.metric", start, end);
                    black_box(db.query(query).unwrap())
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_query_latency);
criterion_main!(benches);
