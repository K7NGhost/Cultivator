use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use cultivator_lib::preview::{image, strings::sqlite_file_format_preview, video};
use std::hint::black_box;

fn sqlite_header() -> [u8; 100] {
    let mut header = [0; 100];
    header[..16].copy_from_slice(b"SQLite format 3\0");
    header[16..18].copy_from_slice(&4096_u16.to_be_bytes());
    header[18] = 1;
    header[19] = 1;
    header[21] = 64;
    header[22] = 32;
    header[23] = 32;
    header[28..32].copy_from_slice(&32_768_u32.to_be_bytes());
    header[40..44].copy_from_slice(&4_u32.to_be_bytes());
    header[56..60].copy_from_slice(&1_u32.to_be_bytes());
    header
}

fn bench_file_format(c: &mut Criterion) {
    let header = sqlite_header();
    let mut group = c.benchmark_group("file_format");
    group.throughput(Throughput::Elements(1));
    group.bench_function("sqlite_header", |b| {
        b.iter(|| sqlite_file_format_preview(black_box(&header)))
    });

    let preview = sqlite_file_format_preview(&header);
    group.bench_function("sqlite_preview_serialization", |b| {
        b.iter(|| serde_json::to_vec(black_box(&preview)).expect("failed to serialize preview"))
    });

    for brand in ["isom", "mp42", "qt  "] {
        let mut header = [0; 24];
        header[..4].copy_from_slice(&24_u32.to_be_bytes());
        header[4..8].copy_from_slice(b"ftyp");
        header[8..12].copy_from_slice(brand.as_bytes());
        group.bench_function(BenchmarkId::new("video_header", brand.trim()), |b| {
            b.iter(|| video::preview(black_box("evidence/clip.mp4"), black_box(&header)))
        });
    }

    for (format, header) in image_headers() {
        group.bench_function(BenchmarkId::new("image_header", format), |b| {
            b.iter(|| image::preview(black_box("evidence/photo"), black_box(&header)))
        });
    }
    group.finish();
}

fn image_headers() -> Vec<(&'static str, Vec<u8>)> {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&13_u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&4032_u32.to_be_bytes());
    png.extend_from_slice(&3024_u32.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0]);

    let mut gif = b"GIF89a".to_vec();
    gif.extend_from_slice(&640_u16.to_le_bytes());
    gif.extend_from_slice(&480_u16.to_le_bytes());
    gif.extend_from_slice(&[0xf7, 0, 0]);

    let mut bmp = vec![0; 54];
    bmp[..2].copy_from_slice(b"BM");
    bmp[2..6].copy_from_slice(&(54_u32 + 1920 * 1080 * 3).to_le_bytes());
    bmp[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bmp[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bmp[18..22].copy_from_slice(&1920_i32.to_le_bytes());
    bmp[22..26].copy_from_slice(&1080_i32.to_le_bytes());
    bmp[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&24_u16.to_le_bytes());

    // JPEG with an APP1 segment ahead of the SOF0 frame header, like camera output.
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
    let app1_payload = vec![0; 4 * 1024];
    jpeg.extend_from_slice(&((app1_payload.len() + 2) as u16).to_be_bytes());
    jpeg.extend_from_slice(&app1_payload);
    jpeg.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 0x08]);
    jpeg.extend_from_slice(&3024_u16.to_be_bytes());
    jpeg.extend_from_slice(&4032_u16.to_be_bytes());
    jpeg.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
    jpeg.extend_from_slice(&[0xff, 0xd9]);

    vec![("png", png), ("gif", gif), ("bmp", bmp), ("jpeg", jpeg)]
}

criterion_group!(benches, bench_file_format);
criterion_main!(benches);
