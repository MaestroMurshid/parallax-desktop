//! Frozen placement (§5.1). A new entry solves against a fixed field: take its
//! nearest neighbours by similarity, compute a target from their positions, and
//! walk outward until it finds empty space. Nothing already placed ever moves.
//!
//! Port of `lib/scene/placement.ts`. The seeded corpus's coordinates come from
//! that implementation, so the two have to agree.

#[derive(Debug, Clone, PartialEq)]
pub struct PlacedNode {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub half_w: f64,
    pub half_h: f64,
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

pub fn place(
    _candidate: &Candidate,
    _field: &[PlacedNode],
    _vectors: &std::collections::HashMap<String, Vec<f32>>,
    _opts: Options,
) -> Placement {
    todo!("placement")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn node(id: &str, x: f64, y: f64) -> PlacedNode {
        PlacedNode { id: id.into(), x, y, half_w: 40.0, half_h: 12.0 }
    }

    fn candidate(vec: Vec<f32>) -> Candidate {
        Candidate { id: "new".into(), vec, half_w: 40.0, half_h: 12.0, links: vec![] }
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
        assert!(from_centre > 60.0, "landed at {from_centre:.1} from the field centre");
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
        assert!(p.x < 500.0, "should sit nearer the stronger match, got x={}", p.x);
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
        assert_eq!(p.anchors.first().unwrap(), "linked", "the link should rank first");
    }

    /// Titles must not sit on top of each other. Collision is the box, not a
    /// circle drawn around it.
    #[test]
    fn it_never_lands_on_top_of_an_existing_node() {
        let opts = Options::default();
        let field: Vec<PlacedNode> = (0..12)
            .map(|i| node(&format!("n{i}"), (i % 4) as f64 * 20.0, (i / 4) as f64 * 20.0))
            .collect();
        let mut vectors = HashMap::new();
        for n in &field {
            vectors.insert(n.id.clone(), a());
        }

        let p = place(&candidate(a()), &field, &vectors, opts);

        for n in &field {
            assert!(!overlaps(&p, n, opts), "overlaps {} at ({}, {})", n.id, p.x, p.y);
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
