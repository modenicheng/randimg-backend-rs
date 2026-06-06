use rayon::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};

static HAMERLY_SKIPS: AtomicUsize = AtomicUsize::new(0);

#[doc(hidden)]
pub fn hamerly_skip_count() -> usize {
    HAMERLY_SKIPS.load(Ordering::Relaxed)
}

#[doc(hidden)]
pub fn reset_hamerly_skips() {
    HAMERLY_SKIPS.store(0, Ordering::Relaxed);
}

struct PointBounds {
    upper: Vec<f32>,
    lower: Vec<f32>,
}

/// KMeans with KMeans++ init, empty-cluster recovery, mini-batch, and rayon parallelism.
///
/// - `data`: input points (e.g. LAB colors)
/// - `k`: number of clusters
/// - `max_iter`: maximum iterations
/// - `batch_size`: if Some(n), use mini-batch KMeans with n samples per iteration;
///   if None, use full-batch KMeans
///
/// Returns `(centroids, counts)` where `counts[j]` is the number of points assigned to cluster `j`.
pub fn kmeans(
    data: &[[f32; 3]],
    k: usize,
    max_iter: usize,
    batch_size: Option<usize>,
    hamerly: bool,
) -> (Vec<[f32; 3]>, Vec<usize>) {
    if data.is_empty() || k == 0 {
        return (vec![], vec![]);
    }
    if data.len() <= k {
        // Not enough data for k clusters — duplicate points to fill
        let mut result = data.to_vec();
        while result.len() < k {
            result.push(data[result.len() % data.len()]);
        }
        let counts = vec![1usize; k];
        return (result, counts);
    }

    // KMeans++ initialization
    let mut centroids = kmeans_pp_init(data, k);

    let mut assignments = vec![0usize; data.len()];

    match batch_size {
        Some(bs) => mini_batch(data, &mut centroids, &mut assignments, k, max_iter, bs),
        None => full_batch(data, &mut centroids, &mut assignments, k, max_iter, hamerly),
    }

    let counts = assignments
        .par_iter()
        .fold(
            || vec![0usize; k],
            |mut c, &a| {
                c[a] += 1;
                c
            },
        )
        .reduce(
            || vec![0usize; k],
            |mut a, b| {
                for i in 0..k {
                    a[i] += b[i];
                }
                a
            },
        );

    (centroids, counts)
}

/// KMeans++ initialization: pick centroids proportional to squared distance from nearest existing centroid.
fn kmeans_pp_init(data: &[[f32; 3]], k: usize) -> Vec<[f32; 3]> {
    // Use deterministic seed: pick the point closest to the dataset centroid
    let mean = par_mean(data);
    let seed_idx = data
        .par_iter()
        .enumerate()
        .map(|(i, p)| (i, hyab_dist(p, &mean)))
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0);

    let mut centroids = Vec::with_capacity(k);
    centroids.push(data[seed_idx]);

    // Precompute min squared distances to nearest chosen centroid
    let mut min_dists = par_min_dists(data, &centroids);

    for _ in 1..k {
        // Pick next centroid weighted by squared distance (deterministic: pick the farthest point)
        let best_idx = min_dists
            .par_iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);

        centroids.push(data[best_idx]);

        // Update min_dists incrementally
        let new_centroid = centroids.last().unwrap();
        min_dists
            .par_iter_mut()
            .zip(data.par_iter())
            .for_each(|(md, point)| {
                let d = hyab_dist(point, new_centroid);
                if d < *md {
                    *md = d;
                }
            });
    }

    centroids
}

fn init_bounds(data: &[[f32; 3]], centroids: &[[f32; 3]], assignments: &[usize]) -> PointBounds {
    let n = data.len();
    let k = centroids.len();
    let mut bounds = PointBounds {
        upper: vec![0.0; n],
        lower: vec![f32::MAX; n],
    };

    data.par_iter()
        .zip(assignments.par_iter())
        .zip(bounds.upper.par_iter_mut().zip(bounds.lower.par_iter_mut()))
        .for_each(|((point, _assigned), (upper, lower))| {
            let mut min1 = f32::MAX;
            let mut min2 = f32::MAX;
            for (_j, c) in centroids.iter().enumerate() {
                let d = hyab_dist(point, c);
                if d < min1 {
                    min2 = min1;
                    min1 = d;
                } else if d < min2 {
                    min2 = d;
                }
            }
            *upper = min1;
            *lower = if k > 1 { min2 } else { 0.0 };
        });

    bounds
}

fn hamerly_assign(
    data: &[[f32; 3]],
    centroids: &[[f32; 3]],
    assignments: &mut [usize],
    bounds: &mut PointBounds,
) -> bool {
    let k = centroids.len();
    if k <= 1 {
        return false;
    }

    let pairwise_dists: Vec<f32> = (0..k)
        .flat_map(|i| {
            (0..k).map(move |j| {
                if i == j {
                    0.0
                } else {
                    hyab_dist(&centroids[i], &centroids[j])
                }
            })
        })
        .collect();

    let chunk_size = 4096;
    let changed = data
        .par_chunks(chunk_size)
        .zip(assignments.par_chunks_mut(chunk_size))
        .zip(bounds.upper.par_chunks_mut(chunk_size))
        .zip(bounds.lower.par_chunks_mut(chunk_size))
        .enumerate()
        .map(
            |(_chunk_idx, (((data_chunk, assign_chunk), upper_chunk), lower_chunk))| {
                let mut local_changed = false;

                for (i_local, point) in data_chunk.iter().enumerate() {
                    let assigned = assign_chunk[i_local];

                    let min_dist_to_other = (0..k)
                        .filter(|&j| j != assigned)
                        .map(|j| pairwise_dists[assigned * k + j])
                        .fold(f32::MAX, f32::min);
                    let p = 0.5 * min_dist_to_other;

                    if upper_chunk[i_local] <= p {
                        HAMERLY_SKIPS.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }

                    let mut min1 = f32::MAX;
                    let mut min2 = f32::MAX;
                    let mut best = 0;
                    for (j, c) in centroids.iter().enumerate() {
                        let d = hyab_dist(point, c);
                        if d < min1 {
                            min2 = min1;
                            min1 = d;
                            best = j;
                        } else if d < min2 {
                            min2 = d;
                        }
                    }

                    upper_chunk[i_local] = min1;
                    lower_chunk[i_local] = min2;

                    if assign_chunk[i_local] != best {
                        assign_chunk[i_local] = best;
                        local_changed = true;
                    }
                }
                local_changed
            },
        )
        .reduce(|| false, |a, b| a || b);

    changed
}

fn update_bounds_after_centroid_move(
    bounds: &mut PointBounds,
    assignments: &[usize],
    old_centroids: &[[f32; 3]],
    new_centroids: &[[f32; 3]],
) {
    let movements: Vec<f32> = old_centroids
        .iter()
        .zip(new_centroids.iter())
        .map(|(old, new)| hyab_dist(old, new))
        .collect();
    let max_movement = movements.iter().cloned().fold(0.0f32, f32::max);

    bounds
        .upper
        .par_iter_mut()
        .zip(bounds.lower.par_iter_mut())
        .zip(assignments.par_iter())
        .for_each(|((upper, lower), &assigned)| {
            *upper += movements[assigned];
            *lower -= max_movement;
            if *lower < 0.0 {
                *lower = 0.0;
            }
        });
}

/// Full-batch KMeans with rayon parallelism.
fn full_batch(
    data: &[[f32; 3]],
    centroids: &mut Vec<[f32; 3]>,
    assignments: &mut [usize],
    k: usize,
    max_iter: usize,
    hamerly: bool,
) {
    if hamerly {
        let mut bounds = init_bounds(data, centroids, assignments);

        for _ in 0..max_iter {
            let old_centroids = centroids.clone();
            let changed = hamerly_assign(data, centroids, assignments, &mut bounds);

            if !changed {
                break;
            }

            let (sums, counts) = par_accumulate(data, assignments, k);
            update_centroids(centroids, &sums, &counts, data, assignments);
            update_bounds_after_centroid_move(&mut bounds, assignments, &old_centroids, centroids);
        }
    } else {
        for _ in 0..max_iter {
            let changed = assignments
                .par_chunks_mut(4096)
                .zip(data.par_chunks(4096))
                .map(|(assign_chunk, data_chunk)| {
                    let mut local_changed = false;
                    for (a, point) in assign_chunk.iter_mut().zip(data_chunk.iter()) {
                        let best = nearest_centroid(point, centroids);
                        if *a != best {
                            *a = best;
                            local_changed = true;
                        }
                    }
                    local_changed
                })
                .reduce(|| false, |a, b| a || b);

            if !changed {
                break;
            }

            let (sums, counts) = par_accumulate(data, assignments, k);
            update_centroids(centroids, &sums, &counts, data, assignments);
        }
    }
}

/// Mini-batch KMeans: sample a batch per iteration for faster convergence.
fn mini_batch(
    data: &[[f32; 3]],
    centroids: &mut Vec<[f32; 3]>,
    assignments: &mut [usize],
    k: usize,
    max_iter: usize,
    batch_size: usize,
) {
    let n = data.len();
    let bs = batch_size.min(n);

    // Pre-select batch indices (deterministic: stride-based sampling)
    let stride = (n / bs).max(1);
    let batch_indices: Vec<usize> = (0..bs).map(|i| (i * stride) % n).collect();

    // Track per-centroid learning rate (number of points assigned to each)
    let mut centroid_counts = vec![0u64; k];

    for _ in 0..max_iter {
        // Assign batch points to nearest centroid
        let batch_changed: bool = batch_indices
            .par_iter()
            .map(|&idx| {
                let point = &data[idx];
                let best = nearest_centroid(point, centroids);
                let old = assignments[idx];
                // We can't mutate assignments in parallel easily, so collect changes
                (idx, best, old != best)
            })
            .collect::<Vec<_>>()
            .into_iter()
            .fold(false, |acc, (idx, best, changed)| {
                assignments[idx] = best;
                acc || changed
            });

        if !batch_changed {
            break;
        }

        // Incrementally update centroids using batch assignments
        for &idx in &batch_indices {
            let c = assignments[idx];
            centroid_counts[c] += 1;
            let eta = 1.0f32 / centroid_counts[c] as f32;
            centroids[c][0] += eta * (data[idx][0] - centroids[c][0]);
            centroids[c][1] += eta * (data[idx][1] - centroids[c][1]);
            centroids[c][2] += eta * (data[idx][2] - centroids[c][2]);
        }

        // Reinitialize empty centroids
        reinitialize_empty_centroids(centroids, &centroid_counts, data, assignments);
    }

    // Final full assignment for accurate results
    assignments
        .par_chunks_mut(4096)
        .zip(data.par_chunks(4096))
        .for_each(|(assign_chunk, data_chunk)| {
            for (a, point) in assign_chunk.iter_mut().zip(data_chunk.iter()) {
                *a = nearest_centroid(point, centroids);
            }
        });

    // Post-processing: use median for L channel (matches HyAB's |ΔL| component)
    let mut l_values: Vec<Vec<f32>> = vec![Vec::new(); k];
    for (point, &c) in data.iter().zip(assignments.iter()) {
        l_values[c].push(point[0]);
    }
    for j in 0..k {
        if !l_values[j].is_empty() {
            centroids[j][0] = median(&mut l_values[j]);
        }
    }
}

#[inline]
fn nearest_centroid(point: &[f32; 3], centroids: &[[f32; 3]]) -> usize {
    let mut min_dist = f32::MAX;
    let mut min_idx = 0;
    for (i, c) in centroids.iter().enumerate() {
        let dist = hyab_dist(point, c);
        if dist < min_dist {
            min_dist = dist;
            min_idx = i;
        }
    }
    min_idx
}

/// Parallel mean of all points.
fn par_mean(data: &[[f32; 3]]) -> [f32; 3] {
    let (sum, count) = data
        .par_iter()
        .fold(
            || ([0.0f32; 3], 0usize),
            |(mut s, c), p| {
                s[0] += p[0];
                s[1] += p[1];
                s[2] += p[2];
                (s, c + 1)
            },
        )
        .reduce(
            || ([0.0f32; 3], 0usize),
            |(mut s1, c1), (s2, c2)| {
                s1[0] += s2[0];
                s1[1] += s2[1];
                s1[2] += s2[2];
                (s1, c1 + c2)
            },
        );

    if count > 0 {
        let n = count as f32;
        [sum[0] / n, sum[1] / n, sum[2] / n]
    } else {
        [0.0f32; 3]
    }
}

/// Parallel min squared distance from each point to the nearest centroid.
fn par_min_dists(data: &[[f32; 3]], centroids: &[[f32; 3]]) -> Vec<f32> {
    data.par_iter()
        .map(|p| {
            centroids
                .iter()
                .map(|c| hyab_dist(p, c))
                .fold(f32::MAX, f32::min)
        })
        .collect()
}

/// Parallel accumulation of sums and counts per cluster.
fn par_accumulate(
    data: &[[f32; 3]],
    assignments: &[usize],
    k: usize,
) -> (Vec<[f32; 3]>, Vec<usize>) {
    data.par_iter()
        .zip(assignments.par_iter())
        .fold(
            || (vec![[0.0f32; 3]; k], vec![0usize; k]),
            |(mut sums, mut counts), (point, &c)| {
                sums[c][0] += point[0];
                sums[c][1] += point[1];
                sums[c][2] += point[2];
                counts[c] += 1;
                (sums, counts)
            },
        )
        .reduce(
            || (vec![[0.0f32; 3]; k], vec![0usize; k]),
            |(mut sums1, mut counts1), (sums2, counts2)| {
                for j in 0..k {
                    sums1[j][0] += sums2[j][0];
                    sums1[j][1] += sums2[j][1];
                    sums1[j][2] += sums2[j][2];
                    counts1[j] += counts2[j];
                }
                (sums1, counts1)
            },
        )
}

/// Update centroids from accumulated sums/counts; reinitialize empty clusters.
/// L channel (index 0) uses median; a/b channels (indices 1,2) use mean.
fn update_centroids(
    centroids: &mut Vec<[f32; 3]>,
    sums: &[[f32; 3]],
    counts: &[usize],
    data: &[[f32; 3]],
    assignments: &[usize],
) {
    let k = centroids.len();
    let mut l_values: Vec<Vec<f32>> = vec![Vec::new(); k];
    for (point, &c) in data.iter().zip(assignments.iter()) {
        l_values[c].push(point[0]);
    }

    for j in 0..k {
        if counts[j] > 0 {
            let n = counts[j] as f32;
            let l_median = median(&mut l_values[j]);
            centroids[j] = [l_median, sums[j][1] / n, sums[j][2] / n];
        }
    }
    reinitialize_empty_centroids(
        centroids,
        &counts.iter().map(|&c| c as u64).collect::<Vec<_>>(),
        data,
        assignments,
    );
}

/// Reinitialize empty centroids to the farthest point from any existing centroid.
fn reinitialize_empty_centroids(
    centroids: &mut Vec<[f32; 3]>,
    counts: &[u64],
    data: &[[f32; 3]],
    assignments: &[usize],
) {
    for j in 0..centroids.len() {
        if counts[j] == 0 {
            // Find the point with the largest distance to its assigned centroid
            // (i.e. the most "poorly served" point)
            let farthest = data
                .par_iter()
                .zip(assignments.par_iter())
                .enumerate()
                .map(|(i, (point, &c))| (i, hyab_dist(point, &centroids[c])))
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
                .unwrap_or(0);
            centroids[j] = data[farthest];
        }
    }
}

fn median(values: &mut [f32]) -> f32 {
    let n = values.len();
    if n == 0 {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if n % 2 == 0 {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    } else {
        values[n / 2]
    }
}

/// HyAB distance metric for CIELAB color space.
/// Better than Euclidean for large color differences (palette extraction).
/// Treats lightness (L*) as separable from chromaticity (a*, b*).
#[inline]
fn hyab_dist(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    let dl = (a[0] - b[0]).abs();
    let da = a[1] - b[1];
    let db = a[2] - b[2];
    dl + (da * da + db * db).sqrt()
}
