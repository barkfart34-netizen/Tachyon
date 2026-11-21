//! Breadcrumb trail: where the player walked, a point every few blocks, kept per dimension and saved. Paired
//! with the death waypoint it answers "how did I get here" and lets the player walk back the same way.

use std::collections::VecDeque;

pub struct Trail {
    pub points: VecDeque<[i32; 3]>,
    pub cap: usize,
    /// Squared distance (blocks) before a new point is kept.
    pub min_step2: i64,
    pub generation: u64,
}

impl Trail {
    pub fn new(cap: usize, min_step: i32) -> Self {
        Trail { points: VecDeque::with_capacity(cap), cap, min_step2: (min_step as i64).pow(2), generation: 0 }
    }

    /// Records a position; returns whether a point was added. A jump over 128 blocks (teleport, respawn) starts a
    /// new segment, marked by a point with `y == i32::MIN`.
    pub fn push(&mut self, p: [i32; 3]) -> bool {
        if let Some(last) = self.points.back().copied().filter(|l| l[1] != i32::MIN) {
            let d = |a: i32, b: i32| (a as i64 - b as i64).pow(2);
            let d2 = d(p[0], last[0]) + d(p[1], last[1]) + d(p[2], last[2]);
            if d2 < self.min_step2 {
                return false;
            }
            if d2 > 128 * 128 {
                self.raw_push([p[0], i32::MIN, p[2]]);
            }
        }
        self.raw_push(p);
        true
    }

    fn raw_push(&mut self, p: [i32; 3]) {
        if self.points.len() >= self.cap {
            self.points.pop_front();
        }
        self.points.push_back(p);
        self.generation += 1;
    }

    pub fn clear(&mut self) {
        self.points.clear();
        self.generation += 1;
    }

    /// Consecutive point pairs of the same segment, newest last.
    pub fn segments(&self) -> impl Iterator<Item = ([i32; 3], [i32; 3])> + '_ {
        self.points
            .iter()
            .zip(self.points.iter().skip(1))
            .filter(|(a, b)| a[1] != i32::MIN && b[1] != i32::MIN)
            .map(|(a, b)| (*a, *b))
    }

    pub fn to_text(&self) -> String {
        let mut s = String::with_capacity(self.points.len() * 16);
        for p in &self.points {
            if p[1] == i32::MIN {
                s.push_str(&format!("- {} {}\n", p[0], p[2]));
            } else {
                s.push_str(&format!("{} {} {}\n", p[0], p[1], p[2]));
            }
        }
        s
    }

    pub fn from_text(&mut self, text: &str) {
        self.points.clear();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("- ") {
                let v: Vec<i32> = rest.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                if v.len() == 2 {
                    self.raw_push([v[0], i32::MIN, v[1]]);
                }
                continue;
            }
            let v: Vec<i32> = line.split_whitespace().filter_map(|t| t.parse().ok()).collect();
            if v.len() == 3 {
                self.raw_push([v[0], v[1], v[2]]);
            }
        }
    }
}
