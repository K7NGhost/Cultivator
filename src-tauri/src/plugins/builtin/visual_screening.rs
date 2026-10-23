use super::{BuiltinPluginContext, MediaGallery, MediaMetadataField};
use image::{
    imageops::{self, FilterType},
    RgbImage,
};
use rayon::prelude::*;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock, RwLock,
    },
    time::Instant,
};

const JOB_MEMORY: u64 = 64 * 1024 * 1024;
const HASH_RADIUS: u32 = 8;
const PARTS: usize = 9;

pub(super) fn analyze(
    gallery: &mut MediaGallery,
    context: &mut BuiltinPluginContext<'_>,
) -> Result<(), String> {
    let enabled = |key| {
        context
            .options
            .get(key)
            .is_some_and(|v| v.as_bool() == Some(true) || v.as_str() == Some("true"))
    };
    let (skin, hashes) = (enabled("screenSkin"), enabled("groupSimilar"));
    if (!skin && !hashes) || gallery.photos.is_empty() {
        return Ok(());
    }
    let workers = super::analysis_limits(context).0;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .thread_name(|i| format!("visual-analysis-{i}"))
        .build()
        .map_err(|e| e.to_string())?;
    // Installed compatibility packages may supply native HEIF libraries. No
    // interpreter is started and no environment is provisioned for screening.
    let runtime = context
        .python_root
        .as_deref()
        .and_then(crate::scripting::runtime::existing_media);
    let executable = runtime.as_ref().map(|r| r.executable.as_path());
    let gate = RwLock::new(());
    let stopped = AtomicBool::new(false);
    let check = context.cancelled;
    let total = gallery.photos.len();
    let started = Instant::now();
    let mut failed = 0;
    context.progress(0, total as u64, "Native visual analysis");
    for (batch_index, batch) in gallery.photos.chunks_mut(workers * 8).enumerate() {
        let results: Vec<_> = pool.install(|| {
            batch
                .par_iter()
                .map(|item| {
                    if check() {
                        stopped.store(true, Ordering::Release);
                    }
                    let pixels = crate::thumbnails::analysis_pixels(
                        Path::new(&item.media_path),
                        executable,
                        &stopped,
                        &gate,
                        JOB_MEMORY,
                        JOB_MEMORY * workers as u64,
                    )?;
                    Ok::<_, String>(measure(&pixels, skin, hashes))
                })
                .collect()
        });
        if stopped.load(Ordering::Acquire) || check() {
            return Err("Plugin run cancelled.".into());
        }
        for (item, result) in batch.iter_mut().zip(results) {
            match result {
                Ok(features) => {
                    if skin {
                        item.skin_ratio = features.skin;
                        item.metadata.push(MediaMetadataField {
                            label: "Skin-tone ratio".into(),
                            value: format!("{:.2}%", features.skin.unwrap_or(0.0) * 100.0),
                        });
                    }
                    if hashes {
                        item.perceptual_hash = features.hashes.map(|(p, _)| format!("{p:016x}"));
                        item.difference_hash = features.hashes.map(|(_, d)| format!("{d:016x}"));
                    }
                    item.metadata.push(MediaMetadataField {
                        label: "Visual analysis engine".into(),
                        value: "native-v2".into(),
                    });
                }
                Err(error) => {
                    failed += 1;
                    item.metadata.push(MediaMetadataField {
                        label: "Visual screening error".into(),
                        value: error,
                    });
                }
            }
        }
        context.progress(
            ((batch_index + 1) * workers * 8).min(total) as u64,
            total as u64,
            format!("Native visual analysis: {failed} failed"),
        );
    }
    let stacks = if hashes {
        group_similar(gallery, &|| check())?
    } else {
        0
    };
    context.log(if failed > 0 { "warn" } else { "info" }, format!("Native visual analysis checked {total} images in {:.2}s; {workers} workers; {stacks} representative-based stacks; {failed} failed. Skin-color matching is not a content classification.", started.elapsed().as_secs_f64()));
    Ok(())
}

struct Features {
    skin: Option<f64>,
    hashes: Option<(u64, u64)>,
}

fn measure(pixels: &RgbImage, skin: bool, hashes: bool) -> Features {
    let ratio = skin.then(|| {
        let matched = pixels.pixels().filter(|p| skin_color(p.0)).count();
        matched as f64 / (u64::from(pixels.width()) * u64::from(pixels.height())).max(1) as f64
    });
    Features {
        skin: ratio,
        hashes: hashes.then(|| fingerprint(pixels)).flatten(),
    }
}

fn skin_color([r, g, b]: [u8; 3]) -> bool {
    let (r, g, b) = (r as f32, g as f32, b as f32);
    // Reject neutral colors and saturated backgrounds before normalization.
    // Relative chromaticity makes the color region less sensitive to exposure.
    if r <= g + 5.0 || r <= b + 10.0 {
        return false;
    }
    let brightness = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    if !(20.0..=245.0).contains(&brightness) {
        return false;
    }
    let sum = r + g + b;
    let red = (r / sum - 0.43) / 0.13;
    let green = (g / sum - 0.32) / 0.085;
    red * red + green * green <= 1.0
}

fn cosine_basis() -> &'static [[f32; 32]; 9] {
    static BASIS: OnceLock<[[f32; 32]; 9]> = OnceLock::new();
    BASIS.get_or_init(|| {
        std::array::from_fn(|k| {
            std::array::from_fn(|n| {
                (std::f32::consts::PI / 32.0 * (n as f32 + 0.5) * k as f32).cos()
            })
        })
    })
}

fn fingerprint(pixels: &RgbImage) -> Option<(u64, u64)> {
    let gray = imageops::grayscale(pixels);
    let sample = imageops::resize(&gray, 32, 32, FilterType::Triangle);
    let values = sample.as_raw();
    let mean = values.iter().map(|&v| v as f32).sum::<f32>() / 1024.0;
    let variance = values
        .iter()
        .map(|&v| (v as f32 - mean).powi(2))
        .sum::<f32>()
        / 1024.0;
    let mut curvature = 0.0;
    for y in 1..31 {
        for x in 1..31 {
            let i = y * 32 + x;
            curvature += (4.0 * values[i] as f32
                - values[i - 1] as f32
                - values[i + 1] as f32
                - values[i - 32] as f32
                - values[i + 32] as f32)
                .abs();
        }
    }
    // Pixel evidence, rather than a hash's bit count, determines whether the
    // image has enough structure. Smooth ramps and flat frames stay unstacked.
    if variance < 36.0 || curvature / 900.0 < 1.2 {
        return None;
    }
    let basis = cosine_basis();
    let mut horizontal = [[0.0f32; 8]; 32];
    for y in 0..32 {
        for k in 0..8 {
            horizontal[y][k] = (0..32)
                .map(|x| values[y * 32 + x] as f32 * basis[k][x])
                .sum();
        }
    }
    // 64 AC coefficients: discard DC so exposure shifts do not dominate the
    // fingerprint. Precomputed separable transforms avoid trigonometry per image.
    let mut coefficients = [0.0f32; 64];
    let mut index = 0;
    for ky in 0..8 {
        for kx in 0..8 {
            if ky == 0 && kx == 0 {
                continue;
            }
            coefficients[index] = (0..32).map(|y| horizontal[y][kx] * basis[ky][y]).sum();
            index += 1;
        }
    }
    coefficients[63] = (0..32).map(|y| horizontal[y][0] * basis[8][y]).sum();
    let mut ordered = coefficients;
    ordered.sort_unstable_by(f32::total_cmp);
    let median = (ordered[31] + ordered[32]) * 0.5;
    let frequency = coefficients
        .iter()
        .fold(0u64, |hash, &v| (hash << 1) | u64::from(v > median));
    let gradient = imageops::resize(&gray, 9, 8, FilterType::Triangle);
    let mut edges = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            edges = (edges << 1)
                | u64::from(gradient.get_pixel(x + 1, y)[0] > gradient.get_pixel(x, y)[0]);
        }
    }
    Some((frequency, edges))
}

fn keys(hash: u64) -> [usize; PARTS] {
    // At most eight changed bits cannot touch all nine disjoint partitions.
    // Thus every candidate within HASH_RADIUS shares at least one bucket.
    std::array::from_fn(|part| {
        part * 256 + ((hash >> (part * 7)) & if part == 8 { 255 } else { 127 }) as usize
    })
}

struct Anchor {
    photo: usize,
    frequency: u64,
    gradient: u64,
    count: usize,
}

fn group_similar(
    gallery: &mut MediaGallery,
    cancelled: &dyn Fn() -> bool,
) -> Result<usize, String> {
    let mut order: Vec<_> = (0..gallery.photos.len()).collect();
    order.sort_unstable_by(|&a, &b| gallery.photos[a].path.cmp(&gallery.photos[b].path));
    let mut frequencies: Vec<Vec<usize>> = (0..PARTS * 256).map(|_| Vec::new()).collect();
    let mut gradients: Vec<Vec<usize>> = (0..PARTS * 256).map(|_| Vec::new()).collect();
    let mut anchors: Vec<Anchor> = Vec::new();
    let mut assignments = Vec::new();
    let mut seen = vec![usize::MAX; order.len()];
    for (stamp, photo) in order.into_iter().enumerate() {
        if cancelled() {
            return Err("Plugin run cancelled.".into());
        }
        let item = &mut gallery.photos[photo];
        item.visual_stack_id = None;
        item.stack_size = 1;
        let pair = item
            .perceptual_hash
            .as_ref()
            .zip(item.difference_hash.as_ref())
            .and_then(|(p, d)| {
                Some((
                    u64::from_str_radix(p, 16).ok()?,
                    u64::from_str_radix(d, 16).ok()?,
                ))
            });
        let Some((frequency, gradient)) = pair else {
            continue;
        };
        let (fk, gk) = (keys(frequency), keys(gradient));
        // Probe the cheaper index. This avoids quadratic frequency buckets when
        // many unrelated frames share a frequency hash but have different edges.
        let frequency_cost: usize = fk.iter().map(|&k| frequencies[k].len()).sum();
        let gradient_cost: usize = gk.iter().map(|&k| gradients[k].len()).sum();
        let (buckets, search) = if frequency_cost <= gradient_cost {
            (&frequencies, fk)
        } else {
            (&gradients, gk)
        };
        let mut best = None;
        let mut best_distance = u32::MAX;
        'candidates: for key in search {
            for &index in &buckets[key] {
                if seen[index] == stamp {
                    continue;
                }
                seen[index] = stamp;
                let anchor = &anchors[index];
                let p = (anchor.frequency ^ frequency).count_ones();
                if p > HASH_RADIUS {
                    continue;
                }
                let d = (anchor.gradient ^ gradient).count_ones();
                if d <= HASH_RADIUS
                    && (p + d < best_distance
                        || (p + d == best_distance && best.is_none_or(|old| index < old)))
                {
                    best = Some(index);
                    best_distance = p + d;
                    if best_distance == 0 {
                        break 'candidates;
                    }
                }
            }
        }
        let anchor = match best {
            Some(index) => {
                anchors[index].count += 1;
                index
            }
            None => {
                let index = anchors.len();
                anchors.push(Anchor {
                    photo,
                    frequency,
                    gradient,
                    count: 1,
                });
                for key in fk {
                    frequencies[key].push(index);
                }
                for key in gk {
                    gradients[key].push(index);
                }
                index
            }
        };
        assignments.push((photo, anchor));
    }
    let names: Vec<_> = anchors
        .iter()
        .map(|a| (a.count > 1).then(|| gallery.photos[a.photo].path.clone()))
        .collect();
    for (photo, anchor) in assignments {
        gallery.photos[photo].visual_stack_id = names[anchor].clone();
        gallery.photos[photo].stack_size = anchors[anchor].count as u64;
    }
    Ok(anchors.iter().filter(|a| a.count > 1).count())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn photo(index: usize, hashes: Option<(u64, u64)>) -> super::super::image_metadata::MediaItem {
        serde_json::from_value(serde_json::json!({
            "id": index.to_string(), "mediaType": "image", "path": format!("/{index:04}.jpg"), "name": "photo.jpg", "format": "JPEG", "size": 1,
            "mediaPath": "unused", "thumbnailPath": "unused", "metadata": [],
            "perceptualHash": hashes.map(|(p, _)| format!("{p:016x}")), "differenceHash": hashes.map(|(_, d)| format!("{d:016x}"))
        })).unwrap()
    }
    fn texture() -> RgbImage {
        let mut seed = 17u64;
        let mut cells = [[0u8; 8]; 8];
        for row in &mut cells {
            for v in row {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                *v = 20 + (seed >> 40) as u8 % 150;
            }
        }
        RgbImage::from_fn(256, 256, |x, y| {
            let v = cells[y as usize / 32][x as usize / 32];
            image::Rgb([v + 20, v, v / 2])
        })
    }
    #[test]
    fn native_color_screening_handles_exposure_and_rejects_neutral_backgrounds() {
        for color in [[240, 205, 190], [180, 120, 95], [70, 43, 27], [40, 31, 26]] {
            assert!(skin_color(color), "{color:?}");
        }
        for color in [
            [0, 0, 255],
            [128, 128, 128],
            [255, 255, 0],
            [255, 0, 0],
            [0, 0, 0],
        ] {
            assert!(!skin_color(color), "{color:?}");
        }
        let pixels = RgbImage::from_fn(128, 64, |x, _| {
            image::Rgb(if x < 64 { [180, 120, 95] } else { [0, 0, 255] })
        });
        assert_eq!(measure(&pixels, true, false).skin, Some(0.5));
    }
    #[test]
    fn native_fingerprints_reject_flat_ramps_and_match_resized_jpeg_copies() {
        assert!(fingerprint(&RgbImage::from_pixel(128, 128, image::Rgb([180, 120, 95]))).is_none());
        let ramp = RgbImage::from_fn(128, 128, |x, _| image::Rgb([x as u8 * 2; 3]));
        assert!(fingerprint(&ramp).is_none());
        let original = texture();
        let small = imageops::resize(&original, 160, 160, FilterType::Triangle);
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 80)
            .encode_image(&small)
            .unwrap();
        let copy = image::load_from_memory(&bytes).unwrap().to_rgb8();
        let (a, b) = (fingerprint(&original).unwrap(), fingerprint(&copy).unwrap());
        assert!((a.0 ^ b.0).count_ones() <= HASH_RADIUS);
        assert!((a.1 ^ b.1).count_ones() <= HASH_RADIUS);
    }
    #[test]
    fn stacks_match_representatives_without_transitive_chains_or_ignored_large_buckets() {
        let mut gallery = MediaGallery {
            photos: vec![
                photo(0, Some((0, 0))),
                photo(1, Some((255, 255))),
                photo(2, Some((65535, 65535))),
                photo(3, None),
                photo(4, None),
            ],
            videos: vec![],
            scanned_files: 5,
        };
        assert_eq!(group_similar(&mut gallery, &|| false).unwrap(), 1);
        assert_eq!(
            gallery.photos[0].visual_stack_id,
            gallery.photos[1].visual_stack_id
        );
        assert!(gallery.photos[2..]
            .iter()
            .all(|p| p.visual_stack_id.is_none()));
        // Eight changed bits may affect every byte. Nine partitions must still
        // find the representative at the inclusive matching boundary.
        gallery.photos = vec![
            photo(0, Some((0, 0))),
            photo(1, Some((0x0101_0101_0101_0101, 0x0101_0101_0101_0101))),
        ];
        assert_eq!(group_similar(&mut gallery, &|| false).unwrap(), 1);
        gallery.photos = (0..2000)
            .map(|i| photo(i, Some((0xaaaa_aaaa_aaaa_aaaa, 0x5555_5555_5555_5555))))
            .collect();
        assert_eq!(group_similar(&mut gallery, &|| false).unwrap(), 1);
        assert!(gallery.photos.iter().all(|p| p.stack_size == 2000));
        assert!(group_similar(&mut gallery, &|| true).is_err());
    }

    #[test]
    fn analysis_runs_without_python_and_keeps_decode_failures_unknown() {
        let directory = tempfile::tempdir().unwrap();
        let mut photos = Vec::new();
        for index in 0..3 {
            let path = directory.path().join(format!("{index}.png"));
            if index < 2 {
                texture().save(&path).unwrap();
            } else {
                std::fs::write(&path, b"broken image").unwrap();
            }
            let mut item = photo(index, None);
            item.media_path = path.to_string_lossy().into_owned();
            photos.push(item);
        }
        let mut gallery = MediaGallery {
            photos,
            videos: vec![],
            scanned_files: 3,
        };
        let mut progress = |_| {};
        let mut context = BuiltinPluginContext::new(
            vec![],
            None,
            serde_json::json!({"screenSkin": true, "groupSimilar": true, "analysisMode": "saver"}),
            vec![],
            &mut progress,
            &|| false,
        );
        analyze(&mut gallery, &mut context).unwrap();
        for item in &gallery.photos[..2] {
            assert!(item.skin_ratio.is_some());
            assert_eq!(item.stack_size, 2);
            assert!(item
                .metadata
                .iter()
                .any(|m| m.label == "Visual analysis engine" && m.value == "native-v2"));
        }
        let failed = &gallery.photos[2];
        assert!(failed.skin_ratio.is_none() && failed.perceptual_hash.is_none());
        assert!(failed.visual_stack_id.is_none());
        assert!(failed
            .metadata
            .iter()
            .any(|m| m.label == "Visual screening error"));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 3);
    }
}
