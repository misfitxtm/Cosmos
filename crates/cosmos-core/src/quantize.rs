// SPDX-License-Identifier: GPL-3.0-or-later
//! Median-cut colour quantisation in OKLab, so splits follow what the eye separates.

use crate::color::{Hex, Oklab};

/// One quantised colour and how many pixels it stands for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cluster {
    pub lab: Oklab,
    pub count: usize,
}

/// Lightness differences are the most visible, so L ranges count a little more.
const L_WEIGHT: f64 = 1.2;

/// Splits the pixels into at most `n` clusters, biggest first.
/// The result depends only on the multiset of pixels, not their order.
pub fn median_cut(pixels: &[Hex], n: usize) -> Vec<Cluster> {
    if pixels.is_empty() || n == 0 {
        return Vec::new();
    }
    let mut buckets = vec![pixels.iter().map(|p| p.to_oklab()).collect::<Vec<_>>()];
    while buckets.len() < n {
        // Split the bucket with the most spread times pixels; none left means all uniform
        let next = (0..buckets.len())
            .map(|i| (i, spread(&buckets[i]).1 * buckets[i].len() as f64))
            .filter(|&(_, v)| v > 0.0)
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)));
        let Some((i, _)) = next else { break };
        let mut bucket = buckets.swap_remove(i);
        let axis = spread(&bucket).0;
        bucket.sort_by(|x, y| axis_value(x, axis).total_cmp(&axis_value(y, axis)));
        let right = bucket.split_off(split_point(&bucket, axis));
        buckets.push(bucket);
        buckets.push(right);
    }

    let mut clusters: Vec<Cluster> = buckets.iter().map(|b| mean(b)).collect();
    clusters.sort_by(|x, y| {
        y.count
            .cmp(&x.count)
            .then(x.lab.l.total_cmp(&y.lab.l))
            .then(x.lab.a.total_cmp(&y.lab.a))
            .then(x.lab.b.total_cmp(&y.lab.b))
    });
    clusters
}

/// The middle index moved to the nearest place the value changes, so identical
/// colours stay in one cluster instead of being cut in half.
fn split_point(sorted: &[Oklab], axis: usize) -> usize {
    let mid = sorted.len() / 2;
    let changes = |i: usize| {
        i > 0
            && i < sorted.len()
            && axis_value(&sorted[i - 1], axis) != axis_value(&sorted[i], axis)
    };
    for d in 0..sorted.len() {
        if mid >= d && changes(mid - d) {
            return mid - d;
        }
        if changes(mid + d) {
            return mid + d;
        }
    }
    mid
}

fn axis_value(c: &Oklab, axis: usize) -> f64 {
    [c.l, c.a, c.b][axis]
}

/// The axis with the widest (weighted) range, and that range.
fn spread(bucket: &[Oklab]) -> (usize, f64) {
    let mut best = (0, 0.0);
    for axis in 0..3 {
        let (lo, hi) = bucket.iter().fold((f64::MAX, f64::MIN), |(lo, hi), c| {
            let v = axis_value(c, axis);
            (lo.min(v), hi.max(v))
        });
        let range = (hi - lo) * if axis == 0 { L_WEIGHT } else { 1.0 };
        if range > best.1 {
            best = (axis, range);
        }
    }
    best
}

fn mean(bucket: &[Oklab]) -> Cluster {
    let n = bucket.len() as f64;
    let sum = bucket
        .iter()
        .fold((0.0, 0.0, 0.0), |s, c| (s.0 + c.l, s.1 + c.a, s.2 + c.b));
    Cluster {
        lab: Oklab {
            l: sum.0 / n,
            a: sum.1 / n,
            b: sum.2 / n,
        },
        count: bucket.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(r: u8, g: u8, b: u8, n: usize) -> Vec<Hex> {
        vec![Hex::new(r, g, b); n]
    }

    #[test]
    fn empty_input_gives_no_clusters() {
        assert!(median_cut(&[], 8).is_empty());
        assert!(median_cut(&px(1, 2, 3, 5), 0).is_empty());
    }

    #[test]
    fn uniform_image_is_one_cluster() {
        let c = median_cut(&px(10, 20, 30, 100), 16);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].count, 100);
    }

    #[test]
    fn separates_distinct_colours_and_counts_add_up() {
        let mut pixels = px(255, 0, 0, 60);
        pixels.extend(px(0, 0, 255, 30));
        pixels.extend(px(20, 20, 20, 10));
        let c = median_cut(&pixels, 3);
        assert_eq!(c.iter().map(|c| c.count).sum::<usize>(), 100);
        assert_eq!(
            c.iter().map(|c| c.count).collect::<Vec<_>>(),
            vec![60, 30, 10]
        );
    }

    #[test]
    fn never_returns_more_than_asked() {
        let pixels: Vec<Hex> = (0..=255).map(|i| Hex::new(i, 255 - i, i / 2)).collect();
        for n in [1, 2, 5, 48, 1000] {
            let c = median_cut(&pixels, n);
            assert!(c.len() <= n && !c.is_empty());
            assert_eq!(c.iter().map(|c| c.count).sum::<usize>(), 256);
        }
    }

    #[test]
    fn a_grey_ramp_still_splits() {
        // a and b are ~0 so only lightness varies
        let pixels: Vec<Hex> = (0..=255).map(|i| Hex::new(i, i, i)).collect();
        assert_eq!(median_cut(&pixels, 8).len(), 8);
    }

    #[test]
    fn pixel_order_does_not_matter() {
        let pixels: Vec<Hex> = (0..200u32)
            .map(|i| {
                Hex::new(
                    (i * 37 % 256) as u8,
                    (i * 91 % 256) as u8,
                    (i * 13 % 256) as u8,
                )
            })
            .collect();
        let mut reversed = pixels.clone();
        reversed.reverse();
        assert_eq!(median_cut(&pixels, 16), median_cut(&reversed, 16));
    }
}
