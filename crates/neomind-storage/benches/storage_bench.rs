//! Storage performance benchmarks using Criterion.rs
//!
//! Run with: cargo bench -p neomind-storage --bench storage_bench

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use neomind_storage::{DataPoint, TimeSeriesStore, VectorDocument, VectorStore};
use tokio::runtime::Runtime;

/// Benchmark time-series write operations
fn bench_timeseries_write(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    c.bench_function("timeseries_write_single_point", |b| {
        b.to_async(&rt).iter(|| async {
            let store = TimeSeriesStore::memory().unwrap();
            let point = DataPoint::new(1234567890, 23.5);
            let _ = store.write("sensor1", "temperature", point).await;
        });
    });

    c.bench_function("timeseries_write_100_points", |b| {
        b.to_async(&rt).iter(|| async {
            let store = TimeSeriesStore::memory().unwrap();
            for i in 0..100 {
                let point = DataPoint::new(1234567890 + i, 20.0 + (i as f64 * 0.1));
                let _ = store.write("sensor1", "temperature", point).await;
            }
        });
    });

    c.bench_function("timeseries_write_1000_points", |b| {
        b.to_async(&rt).iter(|| async {
            let store = TimeSeriesStore::memory().unwrap();
            for i in 0..1000 {
                let point = DataPoint::new(1234567890 + i, 20.0 + (i as f64 * 0.1));
                let _ = store.write("sensor1", "temperature", point).await;
            }
        });
    });
}

/// Benchmark time-series read operations
fn bench_timeseries_read(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    // Setup: create store with 1000 points
    let setup_store = || -> (Runtime, TimeSeriesStore) {
        let rt = Runtime::new().unwrap();
        let store = rt.block_on(async {
            let store = TimeSeriesStore::memory().unwrap();
            for i in 0..1000 {
                let point = DataPoint::new(1234567890 + i, 20.0 + (i as f64 * 0.1));
                let _ = store.write("sensor1", "temperature", point).await.unwrap();
            }
            store
        });
        (rt, store)
    };

    c.bench_function("timeseries_read_last_point", |b| {
        let (rt, store) = setup_store();
        b.to_async(&rt).iter(|| async {
            let _ = store.read_last("sensor1", "temperature").await.unwrap();
        });
    });

    c.bench_function("timeseries_read_range_100", |b| {
        let (rt, store) = setup_store();
        b.to_async(&rt).iter(|| async {
            let _ = store
                .read_range("sensor1", "temperature", 1234567890, 1234567990)
                .await
                .unwrap();
        });
    });
}

/// Benchmark vector search operations
fn bench_vector_search(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    // Setup: create store with vectors
    let setup_store = |size: usize| -> (Runtime, VectorStore) {
        let rt = Runtime::new().unwrap();
        let store = rt.block_on(async {
            let store = VectorStore::new();
            for i in 0..size {
                let embedding: Vec<f32> = (0..128).map(|j| ((i + j) as f32) * 0.01).collect();
                let doc = VectorDocument::new(format!("doc_{}", i), embedding);
                let _ = store.insert(doc).await;
            }
            store
        });
        (rt, store)
    };

    let mut group = c.benchmark_group("vector_search");
    for size in [100, 500, 1000].iter() {
        group.bench_with_input(BenchmarkId::new("search", size), size, |b, &size| {
            let (rt, store) = setup_store(size);
            let query: Vec<f32> = (0..128).map(|i| (i as f32) * 0.01).collect();
            b.to_async(&rt).iter(|| async {
                let _ = store.search(&query, 10).await;
            });
        });
    }
    group.finish();
}

/// Benchmark vector insert operations
fn bench_vector_insert(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    c.bench_function("vector_insert_128d", |b| {
        b.to_async(&rt).iter(|| async {
            let store = VectorStore::new();
            let embedding: Vec<f32> = (0..128).map(|i| (i as f32) * 0.01).collect();
            let doc = VectorDocument::new("test_doc".to_string(), embedding);
            let _ = store.insert(doc).await;
        });
    });

    c.bench_function("vector_insert_512d", |b| {
        b.to_async(&rt).iter(|| async {
            let store = VectorStore::new();
            let embedding: Vec<f32> = (0..512).map(|i| (i as f32) * 0.01).collect();
            let doc = VectorDocument::new("test_doc".to_string(), embedding);
            let _ = store.insert(doc).await;
        });
    });
}

criterion_group!(
    benches,
    bench_timeseries_write,
    bench_timeseries_read,
    bench_vector_search,
    bench_vector_insert,
);
criterion_main!(benches);
