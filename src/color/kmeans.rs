/// Simple KMeans implementation for color clustering
pub fn kmeans(data: &[[f64; 3]], k: usize, max_iter: usize) -> Vec<[f64; 3]> {
    if data.is_empty() || k == 0 {
        return vec![];
    }

    let mut centroids: Vec<[f64; 3]> = Vec::with_capacity(k);
    // Initialize by uniform sampling
    let step = (data.len() / k).max(1);
    for i in 0..k {
        let idx = (i * step).min(data.len() - 1);
        centroids.push(data[idx]);
    }

    let mut assignments = vec![0usize; data.len()];

    for _ in 0..max_iter {
        let mut changed = false;

        // Assign each point to nearest centroid
        for (i, point) in data.iter().enumerate() {
            let mut min_dist = f64::MAX;
            let mut best = 0;
            for (j, centroid) in centroids.iter().enumerate() {
                let dist = euclidean_sq(point, centroid);
                if dist < min_dist {
                    min_dist = dist;
                    best = j;
                }
            }
            if assignments[i] != best {
                assignments[i] = best;
                changed = true;
            }
        }

        if !changed {
            break;
        }

        // Recalculate centroids
        let mut sums = vec![[0.0f64; 3]; k];
        let mut counts = vec![0usize; k];
        for (i, point) in data.iter().enumerate() {
            let c = assignments[i];
            sums[c][0] += point[0];
            sums[c][1] += point[1];
            sums[c][2] += point[2];
            counts[c] += 1;
        }
        for j in 0..k {
            if counts[j] > 0 {
                let n = counts[j] as f64;
                centroids[j] = [sums[j][0] / n, sums[j][1] / n, sums[j][2] / n];
            }
        }
    }

    centroids
}

fn euclidean_sq(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    dx * dx + dy * dy + dz * dz
}
