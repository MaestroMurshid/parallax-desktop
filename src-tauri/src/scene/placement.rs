//! Frozen placement (§5.1). A new entry solves against a fixed field: take its
//! nearest neighbours by similarity, compute a target from their positions, and
//! walk outward until it finds empty space. Nothing already placed ever moves.
//!
//! Port of `lib/scene/placement.ts`. The seeded corpus's coordinates come from
//! that implementation, so the two have to agree.

use crate::scene::vector::{cosine, hash32};
use std::collections::HashMap;
use std::f64::consts::PI;

#[derive(Debug, Clone, PartialEq)]
pub struct PlacedNode {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub half_w: f64,
    pub half_h: f64,
    /// Read by edge placement, which measures the ring against the connected
    /// core rather than the whole field.
    pub isolated: bool,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub vec: Vec<f32>,
    pub half_w: f64,
    pub half_h: f64,
    /// Ids already connected by a named edge. A stated relation outranks a
    /// cosine guess (§5.4), and it is also the only thing that gets drawn --
    /// a line long enough to cross the map is a line nobody can trace.
    pub links: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub strong_threshold: f32,
    pub neighbours: usize,
    pub pad_x: f64,
    pub pad_y: f64,
    pub step: f64,
    pub isolated_gap: f64,
    pub link_weight: f32,
    /// Keeps the outward spiral from stretching into a stringy band.
    pub aspect_cap: f64,
    pub max_steps: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            strong_threshold: 0.55,
            neighbours: 3,
            pad_x: 64.0,
            pad_y: 44.0,
            step: 13.0,
            isolated_gap: 66.0,
            link_weight: 1.25,
            aspect_cap: 2.5,
            max_steps: 4000,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    /// The ids that pulled this entry to where it landed.
    pub anchors: Vec<String>,
    /// Nothing was similar enough, so it went to open ground.
    pub isolated: bool,
}

/// The angle successive isolated entries step by, so a run of them spreads
/// evenly instead of clumping.
fn golden_angle() -> f64 {
    PI * (3.0 - 5.0_f64.sqrt())
}

/// Box collision, not a circle: a circle around wide, short text reserves the
/// empty corners while still colliding horizontally, which reads as scatter.
fn overlaps(x: f64, y: f64, half_w: f64, half_h: f64, field: &[PlacedNode], o: &Options) -> bool {
    field.iter().any(|n| {
        (n.x - x).abs() < n.half_w + half_w + o.pad_x
            && (n.y - y).abs() < n.half_h + half_h + o.pad_y
    })
}

fn centroid_of(field: &[PlacedNode]) -> (f64, f64) {
    let n = field.len() as f64;
    let (sx, sy) = field
        .iter()
        .fold((0.0, 0.0), |(sx, sy), p| (sx + p.x, sy + p.y));
    (sx / n, sy / n)
}

/// Walks outward until the box fits. The spiral starts pointing away from the
/// crowd, so an entry that cannot fit spills off the near edge rather than a
/// random bearing, and steps stretch horizontally to match the shape of a
/// title so overflow lands beside a neighbour instead of under it.
#[allow(clippy::too_many_arguments)]
fn walk_outward(
    target_x: f64,
    target_y: f64,
    half_w: f64,
    half_h: f64,
    field: &[PlacedNode],
    o: &Options,
    away: (f64, f64),
) -> (f64, f64) {
    if !overlaps(target_x, target_y, half_w, half_h, field, o) {
        return (target_x, target_y);
    }

    let dx = target_x - away.0;
    let dy = target_y - away.1;
    let base = if dx == 0.0 && dy == 0.0 {
        0.0
    } else {
        dy.atan2(dx)
    };
    let aspect = o.aspect_cap.min((half_w / half_h.max(1.0)).max(1.0));

    for i in 1..o.max_steps {
        let r = o.step * (i as f64).sqrt();
        let a = base + i as f64 * golden_angle();
        let x = target_x + a.cos() * r * aspect;
        let y = target_y + a.sin() * r;
        if !overlaps(x, y, half_w, half_h, field, o) {
            return (x, y);
        }
    }
    (target_x, target_y)
}

/// §5.1: nothing similar enough goes to open ground at the edge, not to the
/// centroid. Centroid is the tempting default and the wrong one -- it buries an
/// unconnected entry in the densest part of the map, implying a relatedness
/// that is not there.
fn edge_placement(candidate: &Candidate, field: &[PlacedNode], o: &Options) -> (f64, f64) {
    if field.is_empty() {
        return (0.0, 0.0);
    }
    let c = centroid_of(field);

    // Measured against the connected core. Including entries already on this
    // ring made each new one orbit outside the last, so a corpus with several
    // unrelated notes pushed them outward in a widening spiral. One ring, shared.
    let core: Vec<PlacedNode> = field.iter().filter(|n| !n.isolated).cloned().collect();
    let against = if core.is_empty() { field } else { &core };

    let (mut extent_x, mut extent_y) = (0.0_f64, 0.0_f64);
    for n in against {
        extent_x = extent_x.max((n.x - c.0).abs() + n.half_w);
        extent_y = extent_y.max((n.y - c.1).abs() + n.half_h);
    }

    let rank = field.iter().filter(|n| n.isolated).count() as f64;
    // A fixed per-id offset keeps two corpora from looking identical without
    // making the angle arbitrary.
    let jitter = (hash32(&candidate.id) as f64 / u32::MAX as f64) * 0.4;
    let angle = rank * golden_angle() + jitter;

    let rx = extent_x + o.isolated_gap + candidate.half_w;
    let ry = extent_y + o.isolated_gap + candidate.half_h;

    walk_outward(
        c.0 + angle.cos() * rx,
        c.1 + angle.sin() * ry,
        candidate.half_w,
        candidate.half_h,
        field,
        o,
        c,
    )
}

pub fn place(
    candidate: &Candidate,
    field: &[PlacedNode],
    vectors: &HashMap<String, Vec<f32>>,
    opts: Options,
) -> Placement {
    if field.is_empty() {
        return Placement {
            x: 0.0,
            y: 0.0,
            anchors: vec![],
            isolated: true,
        };
    }

    let mut scored: Vec<(&PlacedNode, f32)> = Vec::new();
    for node in field {
        if candidate.links.iter().any(|l| l == &node.id) {
            scored.push((node, opts.link_weight));
            continue;
        }
        let Some(v) = vectors.get(&node.id) else {
            continue;
        };
        let sim = cosine(&candidate.vec, v);
        if sim >= opts.strong_threshold {
            scored.push((node, sim));
        }
    }

    if scored.is_empty() {
        let (x, y) = edge_placement(candidate, field, &opts);
        return Placement {
            x,
            y,
            anchors: vec![],
            isolated: true,
        };
    }

    // Descending by score. Ties keep field order, which is chronological, so
    // the result does not depend on sort stability across implementations.
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(opts.neighbours);

    // Similarity-weighted: an entry that is 0.9 to one note and 0.56 to another
    // belongs near the first, not halfway between them.
    let (mut wx, mut wy, mut wsum) = (0.0, 0.0, 0.0);
    for (node, sim) in &scored {
        let w = (sim - opts.strong_threshold + 0.01) as f64;
        wx += node.x * w;
        wy += node.y * w;
        wsum += w;
    }

    let (x, y) = walk_outward(
        wx / wsum,
        wy / wsum,
        candidate.half_w,
        candidate.half_h,
        field,
        &opts,
        centroid_of(field),
    );

    Placement {
        x,
        y,
        anchors: scored.iter().map(|(n, _)| n.id.clone()).collect(),
        isolated: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn node(id: &str, x: f64, y: f64) -> PlacedNode {
        PlacedNode {
            id: id.into(),
            x,
            y,
            half_w: 40.0,
            half_h: 12.0,
            isolated: false,
        }
    }

    fn candidate(vec: Vec<f32>) -> Candidate {
        Candidate {
            id: "new".into(),
            vec,
            half_w: 40.0,
            half_h: 12.0,
            links: vec![],
        }
    }

    /// Unit vectors, so cosine between them is easy to reason about: `a` and
    /// `b` are orthogonal (0.0), and a mix of the two sits between.
    fn a() -> Vec<f32> {
        vec![1.0, 0.0]
    }
    fn b() -> Vec<f32> {
        vec![0.0, 1.0]
    }

    fn overlaps(p: &Placement, n: &PlacedNode, o: Options) -> bool {
        (p.x - n.x).abs() < 40.0 + n.half_w + o.pad_x
            && (p.y - n.y).abs() < 12.0 + n.half_h + o.pad_y
    }

    #[test]
    fn the_first_entry_lands_at_the_origin() {
        let p = place(&candidate(a()), &[], &HashMap::new(), Options::default());
        assert_eq!(p.x, 0.0);
        assert_eq!(p.y, 0.0);
        assert!(p.isolated, "nothing to be near yet");
        assert!(p.anchors.is_empty());
    }

    /// §5.1: "No strong neighbours -> land in open space at the edge, not at
    /// the centroid of everything." Honest: this does not connect to anything.
    #[test]
    fn nothing_similar_enough_lands_away_from_the_field() {
        let field = vec![node("x", 0.0, 0.0), node("y", 30.0, 10.0)];
        let mut vectors = HashMap::new();
        vectors.insert("x".to_string(), a());
        vectors.insert("y".to_string(), a());

        let p = place(&candidate(b()), &field, &vectors, Options::default());

        assert!(p.isolated);
        assert!(p.anchors.is_empty());
        let from_centre = (p.x * p.x + p.y * p.y).sqrt();
        assert!(
            from_centre > 60.0,
            "landed at {from_centre:.1} from the field centre"
        );
    }

    #[test]
    fn a_strong_neighbour_pulls_it_close_and_is_named_as_an_anchor() {
        let field = vec![node("near", 500.0, 500.0)];
        let mut vectors = HashMap::new();
        vectors.insert("near".to_string(), a());

        let p = place(&candidate(a()), &field, &vectors, Options::default());

        assert!(!p.isolated);
        assert_eq!(p.anchors, vec!["near".to_string()]);
        assert!((p.x - 500.0).abs() < 400.0 && (p.y - 500.0).abs() < 400.0);
    }

    /// The centroid is similarity-weighted, so an entry closer to one
    /// neighbour than the other belongs nearer the first.
    #[test]
    fn the_pull_is_weighted_by_similarity() {
        let field = vec![node("strong", 0.0, 0.0), node("weak", 1000.0, 0.0)];
        let mut vectors = HashMap::new();
        vectors.insert("strong".to_string(), vec![1.0, 0.0]);
        // ~0.58 against the candidate: over the threshold, but only just.
        vectors.insert("weak".to_string(), vec![0.58, 0.815]);

        let p = place(&candidate(a()), &field, &vectors, Options::default());

        assert!(!p.isolated);
        assert!(
            p.x < 500.0,
            "should sit nearer the stronger match, got x={}",
            p.x
        );
    }

    /// §5.4 -- a stated relation is better evidence than a cosine guess, so a
    /// link beats any similarity score.
    #[test]
    fn a_link_outranks_similarity() {
        let field = vec![node("linked", 0.0, 0.0), node("similar", 1000.0, 0.0)];
        let mut vectors = HashMap::new();
        vectors.insert("linked".to_string(), b());
        vectors.insert("similar".to_string(), a());

        let mut c = candidate(a());
        c.links = vec!["linked".to_string()];

        let p = place(&c, &field, &vectors, Options::default());

        assert!(p.anchors.contains(&"linked".to_string()));
        assert_eq!(
            p.anchors.first().unwrap(),
            "linked",
            "the link should rank first"
        );
    }

    /// Titles must not sit on top of each other. Collision is the box, not a
    /// circle drawn around it.
    #[test]
    fn it_never_lands_on_top_of_an_existing_node() {
        let opts = Options::default();
        let field: Vec<PlacedNode> = (0..12)
            .map(|i| {
                node(
                    &format!("n{i}"),
                    (i % 4) as f64 * 20.0,
                    (i / 4) as f64 * 20.0,
                )
            })
            .collect();
        let mut vectors = HashMap::new();
        for n in &field {
            vectors.insert(n.id.clone(), a());
        }

        let p = place(&candidate(a()), &field, &vectors, opts);

        for n in &field {
            assert!(
                !overlaps(&p, n, opts),
                "overlaps {} at ({}, {})",
                n.id,
                p.x,
                p.y
            );
        }
    }

    /// Positions are frozen, so the same corpus placed twice has to produce the
    /// same field -- otherwise nothing is ever where you left it.
    #[test]
    fn placement_is_deterministic() {
        let field = vec![node("x", 0.0, 0.0), node("y", 90.0, 0.0)];
        let mut vectors = HashMap::new();
        vectors.insert("x".to_string(), a());
        vectors.insert("y".to_string(), a());

        let first = place(&candidate(a()), &field, &vectors, Options::default());
        let second = place(&candidate(a()), &field, &vectors, Options::default());
        assert_eq!(first, second);
    }

    /// A node with no vector cannot be scored, and must be skipped rather than
    /// treated as similar.
    #[test]
    fn a_node_without_a_vector_is_not_a_neighbour() {
        let field = vec![node("unvectored", 0.0, 0.0)];
        let p = place(&candidate(a()), &field, &HashMap::new(), Options::default());
        assert!(p.isolated);
        assert!(p.anchors.is_empty());
    }

    /// At most `neighbours` anchors, however many clear the threshold.
    #[test]
    fn no_more_than_three_anchors() {
        let field: Vec<PlacedNode> = (0..8)
            .map(|i| node(&format!("n{i}"), i as f64 * 200.0, 0.0))
            .collect();
        let mut vectors = HashMap::new();
        for n in &field {
            vectors.insert(n.id.clone(), a());
        }

        let p = place(&candidate(a()), &field, &vectors, Options::default());
        assert_eq!(p.anchors.len(), 3);
    }
}
