//! Compression benchmark

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use timeseries_db::compress_gorilla;
use rand::Rng;

fn bench_compression(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression");

    for num_values in [1000, 10_000, 100_000].iter() {
        // Generate realistic time-series data
        let mut values = Vec::with_capacity(*num_values);
        let mut value = 1000.0;
        let mut rng = rand::thread_rng();

        for _ in 0..*num_values {
            value += (rng.gen::<f64>() - 0.5) * 10.0;  // Small changes
            values.push(value);
        }

        // Benchmark compression
        group.bench_with_input(
            BenchmarkId::new("compress", num_values),
            &values,
            |b, values| {
                b.iter(|| {
                    black_box(compress_gorilla(values))
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_compression);
criterion_main!(benches);
