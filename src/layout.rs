use objc2_core_foundation::{CGPoint, CGRect, CGSize};

use crate::settings::Settings;
use crate::shape::DockShape;

/// Where an item's springs are.
#[derive(Clone, Copy, Debug, Default)]
pub struct Motion {
    pub appear: f64,
    pub hover: f64,
    pub press: f64,
    pub active: f64,
}

/// Where an item goes this frame.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub center: f64,
    /// The slot that answers the pointer: the icon's column, spacing and all, the body's height.
    pub hit: CGRect,
    /// The middle of the icon's foot; it scales from there.
    pub icon_base: CGPoint,
    pub icon_scale: (f64, f64),
    pub dot_position: CGPoint,
    pub dot_width: f64,
    pub dot_opacity: f64,
    pub opacity: f64,
}

/// Lays the dock out for a frame: the body from how far it has risen and spread, and each
/// icon in it from its own springs.
pub fn place(settings: &Settings, mid_x: f64, rise: f64, spread: f64, motions: &[Motion]) -> (DockShape, Vec<Placement>) {
    let s = settings;
    let slot = s.icon_size + s.spacing;
    let appears: Vec<f64> = motions.iter().map(|m| m.appear.max(0.0)).collect();
    // Every slot carries the spacing after it, which the row's last icon doesn't need.
    let total: f64 = appears.iter().sum();
    let content_width = (total * slot - s.spacing * total.min(1.0)).max(0.0);
    // The row squeezes with the body, icons and all, so they never stick out of its sides.
    let pinch = s.collapsed_width + (1.0 - s.collapsed_width) * spread;
    let height = s.height() * rise.max(0.0);
    let shape = DockShape {
        mid_x,
        width: (content_width + 2.0 * s.padding) * pinch,
        height,
        corner_radius: s.corner_radius,
        flare_radius: s.flare_radius,
        smoothing: s.smoothing,
    };
    // Icons fade in over the first half of the rise.
    let reveal = (rise * 2.0).clamp(0.0, 1.0);

    let mut x = mid_x - content_width / 2.0;
    let placements = motions
        .iter()
        .zip(&appears)
        .map(|(m, &appear)| {
            let center = mid_x + (x + s.icon_size * appear / 2.0 - mid_x) * pinch;
            x += appear * slot;
            let size = appear * pinch;
            let grow = size * (1.0 + 0.12 * m.hover);
            let fade = appear.min(1.0) * reveal;
            Placement {
                center,
                hit: CGRect::new(
                    CGPoint::new(center - slot * size / 2.0, 0.0),
                    CGSize::new(slot * size, height),
                ),
                // The icon's top rides the body's top edge; hovering lifts it and grows it out of
                // the body, and pressing squashes it onto its foot.
                icon_base: CGPoint::new(center, height - s.top_padding - s.icon_size * size + 4.0 * m.hover),
                icon_scale: (grow * (1.0 + 0.1 * m.press), grow * (1.0 - 0.16 * m.press)),
                dot_position: CGPoint::new(center, height - s.height() + s.bottom_padding * 0.4),
                dot_width: s.dot_size * (1.0 + 2.5 * m.active) * pinch,
                dot_opacity: fade * (0.45 + 0.55 * m.active),
                opacity: fade,
            }
        })
        .collect();
    (shape, placements)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settled(count: usize) -> Vec<Motion> {
        vec![Motion { appear: 1.0, ..Default::default() }; count]
    }

    #[test]
    fn at_rest_the_icons_sit_evenly_inside_the_body() {
        let s = Settings::with_icon_size(48.0);
        let (shape, items) = place(&s, 500.0, 1.0, 1.0, &settled(5));
        assert_eq!(shape.width, s.resting_width(5));
        assert_eq!(shape.height, s.height());
        let left = 500.0 - shape.width / 2.0;
        assert_eq!(items[0].center, left + s.padding + s.icon_size / 2.0);
        for pair in items.windows(2) {
            assert!((pair[1].center - pair[0].center - (s.icon_size + s.spacing)).abs() < 1e-9);
        }
        for item in &items {
            assert_eq!(item.icon_scale, (1.0, 1.0));
            // Top edge of the icon sits top_padding below the body's.
            assert!((item.icon_base.y + s.icon_size - (shape.height - s.top_padding)).abs() < 1e-9);
        }
    }

    #[test]
    fn hit_slots_tile_the_row_without_gaps() {
        let s = Settings::with_icon_size(30.0);
        let (_, items) = place(&s, 0.0, 1.0, 1.0, &settled(4));
        for pair in items.windows(2) {
            let gap = pair[1].hit.origin.x - (pair[0].hit.origin.x + pair[0].hit.size.width);
            assert!(gap.abs() < 1e-9, "gap {gap}");
        }
    }

    #[test]
    fn while_pinched_the_icons_stay_within_the_body() {
        let s = Settings::with_icon_size(48.0);
        for spread in [0.0, 0.3, 0.7] {
            let (shape, items) = place(&s, 400.0, 0.8, spread, &settled(8));
            let body = shape.body_rect();
            for item in &items {
                let half = s.icon_size * item.icon_scale.0 / 2.0;
                assert!(item.center - half >= body.origin.x - 1e-9, "spread {spread}");
                assert!(item.center + half <= body.origin.x + body.size.width + 1e-9, "spread {spread}");
            }
        }
    }

    #[test]
    fn an_arriving_app_opens_its_slot_as_it_appears() {
        let s = Settings::with_icon_size(48.0);
        let mut motions = settled(3);
        motions[2].appear = 0.0;
        let (narrow, _) = place(&s, 0.0, 1.0, 1.0, &motions);
        assert_eq!(narrow.width, s.resting_width(2));
        motions[2].appear = 0.5;
        let (half, _) = place(&s, 0.0, 1.0, 1.0, &motions);
        assert!(half.width > narrow.width && half.width < s.resting_width(3));
    }
}
