use rayon::prelude::*;

/// KMeans with KMeans++ init, empty-cluster recovery, mini-batch, and rayon parallelism.
///
/// - `data`: input points (e.g. LAB colors)
/// - `k`: number of clusters
/// - `max_iter`: maximum iterations
/// - `batch_size`: if Some(n), use mini-batch KMeans with n samples per iteration;
///   if None, use full-batch KMeans
pub fn kmeans(
    data: &[[f64; 3]],
    k: usize,
    max_iter: usize,
    batch_size: Option<usize>,
) -> Vec<[f64; 3]> {
    if data.is_empty() || k == 0 {
        return vec![];
    }
    if data.len() <= k {
        // Not enough data for k clusters — duplicate points to fill
        let mut result = data.to_vec();
        while result.len() < k {
            result.push(data[result.len() % data.len()]);
        }
        return result;
    }

    // KMeans++ initialization
    let mut centroids = kmeans_pp_init(data, k);

    let mut assignments = vec![0usize; data.len()];

    match batch_size {
        Some(bs) => mini_batch(data, &mut centroids, &mut assignments, k, max_iter, bs),
        None => full_batch(data, &mut centroids, &mut assignments, k, max_iter),
    }

    centroids
}

/// KMeans++ initialization: pick centroids proportional to squared distance from nearest existing centroid.
fn kmeans_pp_init(data: &[[f64; 3]], k: usize) -> Vec<[f64; 3]> {
    // Use deterministic seed: pick the point closest to the dataset centroid
    let mean = par_mean(data);
    let seed_idx = data
        .par_iter()
        .enumerate()
        .map(|(i, p)| (i, euclidean_sq(p, &mean)))
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
                let d = euclidean_sq(point, new_centroid);
                if d < *md {
                    *md = d;
                }
            });
    }

    centroids
}

/// Full-batch KMeans with rayon parallelism.
fn full_batch(
    data: &[[f64; 3]],
    centroids: &mut Vec<[f64; 3]>,
    assignments: &mut [usize],
    k: usize,
    max_iter: usize,
) {
    for _ in 0..max_iter {
        // Parallel assignment: each chunk of points finds its nearest centroid
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

        // Parallel centroid accumulation
        let (sums, counts) = par_accumulate(data, assignments, k);

        // Update centroids, reinitialize empty clusters
        update_centroids(centroids, &sums, &counts, data, assignments);
    }
}

/// Mini-batch KMeans: sample a batch per iteration for faster convergence.
fn mini_batch(
    data: &[[f64; 3]],
    centroids: &mut Vec<[f64; 3]>,
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
            let eta = 1.0 / centroid_counts[c] as f64;
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
}

/// Find the index of the nearest centroid to a point.
#[inline]
fn nearest_centroid(point: &[f64; 3], centroids: &[[f64; 3]]) -> usize {
    centroids
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            euclidean_sq(point, a)
                .partial_cmp(&euclidean_sq(point, b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Parallel mean of all points.
fn par_mean(data: &[[f64; 3]]) -> [f64; 3] {
    let (sum, count) = data
        .par_iter()
        .fold(
            || ([0.0f64; 3], 0usize),
            |(mut s, c), p| {
                s[0] += p[0];
                s[1] += p[1];
                s[2] += p[2];
                (s, c + 1)
            },
        )
        .reduce(
            || ([0.0f64; 3], 0usize),
            |(mut s1, c1), (s2, c2)| {
                s1[0] += s2[0];
                s1[1] += s2[1];
                s1[2] += s2[2];
                (s1, c1 + c2)
            },
        );

    if count > 0 {
        let n = count as f64;
        [sum[0] / n, sum[1] / n, sum[2] / n]
    } else {
        [0.0; 3]
    }
}

/// Parallel min squared distance from each point to the nearest centroid.
fn par_min_dists(data: &[[f64; 3]], centroids: &[[f64; 3]]) -> Vec<f64> {
    data.par_iter()
        .map(|p| {
            centroids
                .iter()
                .map(|c| euclidean_sq(p, c))
                .fold(f64::MAX, f64::min)
        })
        .collect()
}

/// Parallel accumulation of sums and counts per cluster.
fn par_accumulate(
    data: &[[f64; 3]],
    assignments: &[usize],
    k: usize,
) -> (Vec<[f64; 3]>, Vec<usize>) {
    data.par_iter()
        .zip(assignments.par_iter())
        .fold(
            || (vec![[0.0f64; 3]; k], vec![0usize; k]),
            |(mut sums, mut counts), (point, &c)| {
                sums[c][0] += point[0];
                sums[c][1] += point[1];
                sums[c][2] += point[2];
                counts[c] += 1;
                (sums, counts)
            },
        )
        .reduce(
            || (vec![[0.0f64; 3]; k], vec![0usize; k]),
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
fn update_centroids(
    centroids: &mut Vec<[f64; 3]>,
    sums: &[[f64; 3]],
    counts: &[usize],
    data: &[[f64; 3]],
    assignments: &[usize],
) {
    for j in 0..centroids.len() {
        if counts[j] > 0 {
            let n = counts[j] as f64;
            centroids[j] = [sums[j][0] / n, sums[j][1] / n, sums[j][2] / n];
        }
    }
    reinitialize_empty_centroids(centroids, &counts.iter().map(|&c| c as u64).collect::<Vec<_>>(), data, assignments);
}

/// Reinitialize empty centroids to the farthest point from any existing centroid.
fn reinitialize_empty_centroids(
    centroids: &mut Vec<[f64; 3]>,
    counts: &[u64],
    data: &[[f64; 3]],
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
                .map(|(i, (point, &c))| (i, euclidean_sq(point, &centroids[c])))
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
                .unwrap_or(0);
            centroids[j] = data[farthest];
        }
    }
}

/// Squared Euclidean distance between two 3D points.
#[inline]
fn euclidean_sq(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    dx * dx + dy * dy + dz * dz
}
