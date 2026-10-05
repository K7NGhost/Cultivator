use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::{
    cell::Cell,
    fs::File,
    hint::black_box,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use tempfile::TempDir;
use tokio::runtime::Runtime;

use cultivator_lib::preview::{open_text_preview_impl, read_text_preview_lines_impl};

fn text_fixture_path(directory: &Path, index: usize, target_size: usize) -> PathBuf {
    let path = directory.join(format!("text_preview_{index}.txt"));
    let file = File::create(&path).expect("failed to create text preview fixture");
    let mut writer = BufWriter::new(file);
    let mut written = 0;
    let mut line_number = 1;

    while written < target_size {
        let line = format!(
            "{line_number:04}: timestamp=2026-07-07T12:00:00Z level=INFO event=text-preview-benchmark payload=abcdefghijklmnopqrstuvwxyz0123456789\n",
        );
        let bytes = line.as_bytes();
        let remaining = (target_size - written).min(bytes.len());
        writer
            .write_all(&bytes[..remaining])
            .expect("failed to write text preview fixture");
        written += remaining;
        line_number += 1;
    }
    writer
        .flush()
        .expect("failed to flush text preview fixture");
    path
}

fn bench_text_preview(c: &mut Criterion) {
    // A single-threaded executor keeps async scheduling inside the measured thread.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to create tokio runtime");
    let fixtures = TempDir::new().expect("failed to create benchmark fixture directory");
    let fixture_paths = (0..8)
        .map(|index| {
            text_fixture_path(fixtures.path(), index, 4 * 1024 * 1024)
                .to_string_lossy()
                .to_string()
        })
        .collect::<Vec<_>>();
    bench_fixture(c, &runtime, "text_preview", "4_mib", fixture_paths);

    if std::env::var("CULTIVATOR_BENCH_LARGE").as_deref() == Ok("1") {
        let path = text_fixture_path(fixtures.path(), 8, 100 * 1024 * 1024)
            .to_string_lossy()
            .to_string();
        bench_fixture(c, &runtime, "text_preview_large", "100_mib", vec![path]);
    }
}

fn bench_fixture(
    c: &mut Criterion,
    runtime: &Runtime,
    group_name: &str,
    fixture_label: &str,
    fixture_paths: Vec<String>,
) {
    let range_path = fixture_paths[0].clone();

    let summary = runtime
        .block_on(open_text_preview_impl(range_path.clone()))
        .expect("failed to warm text preview range benchmark");
    let middle_row = (summary.line_count / 2) as usize;
    let end_row = summary.line_count.saturating_sub(120) as usize;

    let next_fixture_index = Cell::new(0usize);
    let mut group = c.benchmark_group(group_name);
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));

    // Eight small files exceed the four-map cache; the large file measures a warm map.
    let summary_label = if fixture_paths.len() > 4 {
        "mmap_open_summary"
    } else {
        "cached_open_summary"
    };

    group.bench_function(BenchmarkId::new(summary_label, fixture_label), |bencher| {
        bencher.to_async(runtime).iter(|| {
            let index = next_fixture_index.get();
            next_fixture_index.set((index + 1) % fixture_paths.len());
            let path = fixture_paths[index].clone();

            async move {
                open_text_preview_impl(black_box(path))
                    .await
                    .expect("text preview mmap benchmark failed")
            }
        });
    });

    for (label, start_row) in [("start", 0usize), ("middle", middle_row), ("end", end_row)] {
        group.bench_function(BenchmarkId::new("read_visible_range", label), |bencher| {
            bencher.to_async(runtime).iter(|| {
                let path = range_path.clone();

                async move {
                    read_text_preview_lines_impl(black_box(path), black_box(start_row), 120)
                        .await
                        .expect("text preview range benchmark failed")
                }
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_text_preview);
criterion_main!(benches);
