//! Shared geometric coefficients for CPU generation and optional GPU packing.

pub(crate) type Point = (isize, isize);

pub(crate) struct Direction {
    pub(crate) differences: Vec<(usize, usize, f32)>,
    pub(crate) samples: Vec<(usize, usize, f32)>,
}

fn index((x, y): Point) -> usize {
    ((y + 2) * 5 + x + 2) as usize
}

pub(crate) fn direction(pattern: [usize; 4], phase: usize, d: Point) -> Direction {
    let (x, y) = (phase % 2, phase / 2);
    let green = pattern[(y % 2) * 2 + x % 2] == 1;
    let (dx, dy) = d;
    let double = (2 * dx, 2 * dy);
    let mut pairs = vec![(d, (-dx, -dy), 1.0), (double, (0, 0), 1.0)];
    let mut points;
    if dx == 0 || dy == 0 {
        let (px, py) = (-dy, dx);
        points = vec![(0, 0), d, double];
        for sign in [-1, 1] {
            let side = (sign * px, sign * py);
            let near = (dx + side.0, dy + side.1);
            let far = (2 * dx + side.0, 2 * dy + side.1);
            pairs.push((near, (-dx + side.0, -dy + side.1), 0.5));
            pairs.push((far, side, 0.5));
            if green {
                points.extend([side, far]);
            } else {
                points.push(near);
            }
        }
    } else {
        points = vec![d, (dx, 0), (0, dy), (2 * dx, dy), (dx, 2 * dy)];
        if green {
            pairs.extend([((dx, 2 * dy), (-dx, 0), 1.0), ((2 * dx, dy), (0, -dy), 1.0)]);
        } else {
            points.extend([(0, 0), double]);
            pairs.extend([
                ((0, dy), (-dx, 0), 0.5),
                ((dx, 0), (0, -dy), 0.5),
                ((dx, 2 * dy), (0, dy), 0.5),
                ((2 * dx, dy), (dx, 0), 0.5),
            ]);
        }
    }
    let color_at = |(dx, dy): Point| {
        let px = (x as isize + dx).rem_euclid(2) as usize;
        let py = (y as isize + dy).rem_euclid(2) as usize;
        pattern[py * 2 + px]
    };
    let mut counts = [0; 3];
    for &point in &points {
        counts[color_at(point)] += 1;
    }
    debug_assert!(counts.iter().all(|&n| n > 0));
    Direction {
        differences: pairs
            .into_iter()
            .map(|(a, b, w)| (index(a), index(b), w))
            .collect(),
        samples: points
            .into_iter()
            .map(|p| {
                let color = color_at(p);
                (index(p), color, 1.0 / counts[color] as f32)
            })
            .collect(),
    }
}
