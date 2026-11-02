use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, SQRT_2};
use std::ptr;

use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGMutablePath;

/// The dock's outline: a body standing on the bottom edge of the screen, its top corners
/// rounded and its foot flaring out into the edge through concave fillets, the way the notch
/// meets the top of the screen. `y` is 0 at the edge and grows upward.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DockShape {
    pub mid_x: f64,
    pub width: f64,
    pub height: f64,
    pub corner_radius: f64,
    pub flare_radius: f64,
    /// Figma's corner smoothing, 0 for circular corners.
    pub smoothing: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Element {
    Move(CGPoint),
    Line(CGPoint),
    Curve { control1: CGPoint, control2: CGPoint, end: CGPoint },
    Close,
}

impl DockShape {
    /// The body, without the flares at its foot.
    pub fn body_rect(&self) -> CGRect {
        CGRect::new(CGPoint::new(self.mid_x - self.width / 2.0, 0.0), CGSize::new(self.width, self.height))
    }

    pub fn elements(&self) -> Vec<Element> {
        let (width, height) = (self.width, self.height);
        if height <= 0.01 || width <= 0.01 {
            return Vec::new();
        }
        // A corner takes (1 + smoothing)·radius of each edge beside it. While the body is too
        // short for both corners on a side, they shrink together, so it grows out of the edge
        // as a shallow bump.
        let k = 1.0 + self.smoothing;
        let fit = (height / ((self.corner_radius + self.flare_radius) * k)).min(1.0);
        let top = (self.corner_radius * fit).min(width / 2.0 / k);
        let flare = self.flare_radius * fit;
        let (left, right) = (self.mid_x - width / 2.0, self.mid_x + width / 2.0);
        let (up, down, leftward, rightward) = ((0.0, 1.0), (0.0, -1.0), (-1.0, 0.0), (1.0, 0.0));
        let s = self.smoothing;

        let mut elements = vec![
            Element::Move(CGPoint::new(left - flare * k, 0.0)),
            Element::Line(CGPoint::new(right + flare * k, 0.0)),
        ];
        // A flare is the corner of the space outside the body, run backwards.
        Corner::new(CGPoint::new(right, 0.0), down, rightward, flare, s).reversed().append_to(&mut elements);
        elements.push(Element::Line(CGPoint::new(right, height - top * k)));
        Corner::new(CGPoint::new(right, height), up, leftward, top, s).append_to(&mut elements);
        elements.push(Element::Line(CGPoint::new(left + top * k, height)));
        Corner::new(CGPoint::new(left, height), leftward, down, top, s).append_to(&mut elements);
        elements.push(Element::Line(CGPoint::new(left, flare * k)));
        Corner::new(CGPoint::new(left, 0.0), rightward, up, flare, s).reversed().append_to(&mut elements);
        elements.push(Element::Close);
        elements
    }

    pub fn path(&self) -> CFRetained<CGMutablePath> {
        let path = CGMutablePath::new();
        let target = Some(&*path);
        for element in self.elements() {
            // SAFETY: a null transform is allowed, and means none.
            unsafe {
                match element {
                    Element::Move(p) => CGMutablePath::move_to_point(target, ptr::null(), p.x, p.y),
                    Element::Line(p) => CGMutablePath::add_line_to_point(target, ptr::null(), p.x, p.y),
                    Element::Curve { control1: a, control2: b, end: p } => {
                        CGMutablePath::add_curve_to_point(target, ptr::null(), a.x, a.y, b.x, b.y, p.x, p.y)
                    }
                    Element::Close => CGMutablePath::close_subpath(target),
                }
            }
        }
        path
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Segment {
    control1: CGPoint,
    control2: CGPoint,
    end: CGPoint,
}

/// A corner rounded with Figma's corner smoothing: a cubic easing into a circular arc and
/// another easing out of it, so the curvature builds up instead of jumping where the edges
/// end. It runs from `start`, (1 + smoothing)·radius before the vertex along the incoming
/// edge, to as far past it along the outgoing one.
#[derive(Clone, Debug, PartialEq)]
struct Corner {
    start: CGPoint,
    segments: Vec<Segment>,
}

impl Corner {
    /// The corner at `vertex` between an edge heading along `u` into it and one heading along
    /// `v` out of it.
    fn new(vertex: CGPoint, u: (f64, f64), v: (f64, f64), r: f64, s: f64) -> Self {
        let offset = |point: CGPoint, along: f64, across: f64| {
            CGPoint::new(point.x + along * u.0 + across * v.0, point.y + along * u.1 + across * v.1)
        };
        let p = (1.0 + s) * r;
        let start = offset(vertex, -p, 0.0);
        if r <= 0.001 {
            return Self { start, segments: Vec::new() };
        }
        let arc_angle = FRAC_PI_2 * (1.0 - s);
        let arc_side = (arc_angle / 2.0).sin() * r * SQRT_2;
        let tangent = r * ((FRAC_PI_2 - arc_angle) / 4.0).tan();
        let beta = FRAC_PI_4 * s;
        let c = tangent * beta.cos();
        let d = c * beta.tan();
        let b = (p - arc_side - c - d) / 3.0;
        let a = 2.0 * b;

        let arc_start = offset(start, a + b + c, d);
        let arc_end = offset(arc_start, arc_side, arc_side);
        // The arc as one cubic: its unit tangents at either end, in (u, v), times
        // 4/3·tan(θ/4) of the radius.
        let handle = 4.0 / 3.0 * (arc_angle / 4.0).tan();
        let segments = vec![
            Segment { control1: offset(start, a, 0.0), control2: offset(start, a + b, 0.0), end: arc_start },
            Segment {
                control1: offset(arc_start, (r - d) * handle, (r - arc_side - d) * handle),
                control2: offset(arc_end, -(r - arc_side - d) * handle, -(r - d) * handle),
                end: arc_end,
            },
            Segment { control1: offset(arc_end, d, c), control2: offset(arc_end, d, b + c), end: offset(arc_end, d, a + b + c) },
        ];
        Self { start, segments }
    }

    fn end(&self) -> CGPoint {
        self.segments.last().map_or(self.start, |segment| segment.end)
    }

    fn reversed(&self) -> Self {
        let points: Vec<CGPoint> = std::iter::once(self.start).chain(self.segments.iter().map(|s| s.end)).collect();
        let segments = (0..self.segments.len())
            .rev()
            .map(|i| Segment { control1: self.segments[i].control2, control2: self.segments[i].control1, end: points[i] })
            .collect();
        Self { start: self.end(), segments }
    }

    /// Adds the corner's curves; the path must already be at `start`.
    fn append_to(&self, elements: &mut Vec<Element>) {
        elements.extend(
            self.segments
                .iter()
                .map(|s| Element::Curve { control1: s.control1, control2: s.control2, end: s.end }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: CGPoint, b: CGPoint) -> bool {
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
    }

    fn shape(height: f64) -> DockShape {
        DockShape { mid_x: 200.0, width: 300.0, height, corner_radius: 22.0, flare_radius: 12.0, smoothing: 0.6 }
    }

    /// Every point the outline passes through, in order.
    fn points(elements: &[Element]) -> Vec<CGPoint> {
        elements
            .iter()
            .filter_map(|e| match *e {
                Element::Move(p) | Element::Line(p) | Element::Curve { end: p, .. } => Some(p),
                Element::Close => None,
            })
            .collect()
    }

    #[test]
    fn a_corner_runs_from_p_before_the_vertex_to_p_after_it() {
        for s in [0.0, 0.3, 0.6, 1.0] {
            let corner = Corner::new(CGPoint::new(10.0, 10.0), (1.0, 0.0), (0.0, -1.0), 4.0, s);
            let p = (1.0 + s) * 4.0;
            assert!(close(corner.start, CGPoint::new(10.0 - p, 10.0)), "smoothing {s}");
            assert!(close(corner.end(), CGPoint::new(10.0, 10.0 - p)), "smoothing {s}");
        }
    }

    #[test]
    fn the_arc_lies_on_the_corner_circle() {
        let (r, s) = (10.0, 0.6);
        let corner = Corner::new(CGPoint::new(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), r, s);
        // The circle touching both edges is centred r back along u and r along v.
        let centre = CGPoint::new(-r, r);
        for point in [corner.segments[0].end, corner.segments[1].end] {
            let distance = ((point.x - centre.x).powi(2) + (point.y - centre.y).powi(2)).sqrt();
            assert!((distance - r).abs() < 1e-9, "distance {distance}");
        }
    }

    #[test]
    fn reversing_swaps_the_ends() {
        let corner = Corner::new(CGPoint::new(5.0, 0.0), (0.0, -1.0), (1.0, 0.0), 3.0, 0.6);
        let back = corner.reversed();
        assert!(close(back.start, corner.end()));
        assert!(close(back.end(), corner.start));
        assert_eq!(back.reversed(), corner);
    }

    #[test]
    fn the_outline_is_closed_and_flares_into_the_edge() {
        let shape = shape(70.0);
        let elements = shape.elements();
        let points = points(&elements);
        let k = 1.0 + shape.smoothing;
        assert!(close(points[0], CGPoint::new(50.0 - 12.0 * k, 0.0)));
        assert!(close(*points.last().unwrap(), points[0]));
        assert_eq!(elements.last(), Some(&Element::Close));
        let top = points.iter().map(|p| p.y).fold(f64::MIN, f64::max);
        let (min_x, max_x) = points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
        assert_eq!(top, 70.0);
        assert!((min_x - (50.0 - 12.0 * k)).abs() < 1e-9 && (max_x - (350.0 + 12.0 * k)).abs() < 1e-9);
        // The flares meet the body's sides above the edge.
        assert!(points.iter().any(|p| close(*p, CGPoint::new(350.0, 12.0 * k))));
        assert!(points.iter().any(|p| close(*p, CGPoint::new(50.0, 12.0 * k))));
    }

    #[test]
    fn a_short_body_shrinks_its_corners_to_fit() {
        for height in [1.0, 5.0, 20.0, 40.0] {
            let points = points(&shape(height).elements());
            assert!(points.iter().all(|p| p.y >= -1e-9 && p.y <= height + 1e-9), "height {height}");
            // Up each side, the outline never turns back down.
            let right: Vec<f64> = points.iter().filter(|p| (p.x - 350.0).abs() < 1e-9).map(|p| p.y).collect();
            assert!(right.windows(2).all(|w| w[0] <= w[1] + 1e-9), "height {height}: {right:?}");
        }
    }

    #[test]
    fn nothing_is_drawn_while_tucked_away() {
        assert!(shape(0.0).elements().is_empty());
    }
}
