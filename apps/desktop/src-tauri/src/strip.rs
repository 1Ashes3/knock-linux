//! Pure strip geometry (SPEC.md §9.2, §9.3). No OS calls; unit-tested on
//! recorded monitor data.

use serde::Deserialize;

/// Rectangle in physical screen pixels; `right` and `bottom` are exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Gap between the strip's right edge and the work area's right edge (§9.2).
const RIGHT_MARGIN: f64 = 8.0;
/// Most slots the strip is ever wide: 5 characters plus the den (§9.2).
const MAX_SLOTS: f64 = 6.0;

/// Where the strip is and what the cursor poll tests against (§9.3).
pub struct Strip {
    pub rect: Rect,
    pub scale: f64,
    pub regions: Vec<Region>,
}

/// Physical rect of a strip `width` by `height` logical px, resting on the
/// bottom edge of `work_area` and 8 logical px from its right edge, so it
/// grows leftward and upward from that corner.
pub fn place(work_area: Rect, scale: f64, width: f64, height: f64) -> Rect {
    let px = |logical: f64| (logical * scale).round() as i32;
    let right = work_area.right - px(RIGHT_MARGIN);
    Rect {
        left: right - px(width),
        top: work_area.bottom - px(height),
        right,
        bottom: work_area.bottom,
    }
}

/// The size `request_strip_size` gets (§8.5, §9.2), in logical px: at most 6
/// slots wide (a slot is `slot` px, the size setting's strip height) and at
/// most `max_height` tall. Zero is allowed (no sessions); `None` for a
/// negative or non-number size.
pub fn clamp(width: f64, height: f64, slot: f64, max_height: f64) -> Option<(f64, f64)> {
    (width >= 0.0 && height >= 0.0).then(|| (width.min(MAX_SLOTS * slot), height.min(max_height)))
}

/// A hit region from `set_hit_regions` (§8.5): logical px, relative to the
/// strip's top-left.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Whether the physical screen point `cursor` is inside any region of the
/// strip at `strip` (physical) with `scale`.
pub fn hit(regions: &[Region], strip: Rect, scale: f64, cursor: (i32, i32)) -> bool {
    let x = f64::from(cursor.0 - strip.left) / scale;
    let y = f64::from(cursor.1 - strip.top) / scale;
    regions
        .iter()
        .any(|r| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    const MEDIUM: f64 = 64.0;
    const THREE: f64 = 3.0 * MEDIUM;

    // 100%: recorded on the user's machine (1920x1080 monitor, bottom
    // taskbar 1032..1080, Windows 11 25H2). 150% and 200%: not recorded yet;
    // derived from a 48 logical px bottom taskbar (72 and 96 physical px).
    #[test]
    fn place_at_100_percent() {
        let wa = rect(0, 0, 1920, 1032);
        assert_eq!(place(wa, 1.0, THREE, MEDIUM), rect(1720, 968, 1912, 1032));
    }

    #[test]
    fn place_at_150_percent() {
        let wa = rect(0, 0, 2560, 1368);
        // 64*1.5 = 96 tall, 192*1.5 = 288 wide, 8*1.5 = 12 margin.
        assert_eq!(place(wa, 1.5, THREE, MEDIUM), rect(2260, 1272, 2548, 1368));
    }

    #[test]
    fn place_at_200_percent() {
        let wa = rect(0, 0, 3840, 2064);
        assert_eq!(place(wa, 2.0, THREE, MEDIUM), rect(3440, 1936, 3824, 2064));
    }

    #[test]
    fn place_on_offset_monitor_rounds_fractional_scale() {
        // Primary monitor need not start at 0,0; 125% gives fractional px.
        let wa = rect(-1920, 200, 0, 1232);
        // 64*1.25 = 80, 192*1.25 = 240, 8*1.25 = 10.
        assert_eq!(place(wa, 1.25, THREE, MEDIUM), rect(-250, 1152, -10, 1232));
    }

    #[test]
    fn requested_size_is_capped_at_six_slots_and_the_work_area() {
        assert_eq!(clamp(200.0, 64.0, 64.0, 1032.0), Some((200.0, 64.0)));
        assert_eq!(clamp(500.0, 90.0, 64.0, 1032.0), Some((384.0, 90.0)));
        assert_eq!(clamp(1000.0, 48.0, 48.0, 1032.0), Some((288.0, 48.0)));
        assert_eq!(clamp(100.0, 5000.0, 64.0, 1032.0), Some((100.0, 1032.0)));
        let inf = f64::INFINITY;
        assert_eq!(clamp(inf, inf, 80.0, 700.0), Some((480.0, 700.0)));
        assert_eq!(
            clamp(0.0, 64.0, 64.0, 1032.0),
            Some((0.0, 64.0)),
            "no sessions"
        );
        for (w, h) in [
            (-1.0, 64.0),
            (64.0, -0.5),
            (f64::NAN, 64.0),
            (64.0, f64::NAN),
        ] {
            assert_eq!(clamp(w, h, 64.0, 1032.0), None, "{w} {h}");
        }
        // Six medium slots at 150%, right edge kept: 384*1.5 = 576 wide.
        let wa = rect(0, 0, 2560, 1368);
        assert_eq!(place(wa, 1.5, 384.0, 64.0), rect(1972, 1272, 2548, 1368));
    }

    const STRIP_150: Rect = rect(2260, 1272, 2548, 1368);
    const REGION: Region = Region {
        x: 10.0,
        y: 20.0,
        width: 40.0,
        height: 30.0,
    };

    #[test]
    fn hit_converts_physical_cursor_to_logical() {
        let r = [REGION];
        // Logical (10,20) at 150% is physical offset (15,30).
        assert!(hit(&r, STRIP_150, 1.5, (2260 + 15, 1272 + 30)));
        assert!(!hit(&r, STRIP_150, 1.5, (2260 + 14, 1272 + 30)));
        // Right/bottom edges are exclusive: logical 50,50 -> physical 75,75.
        assert!(hit(&r, STRIP_150, 1.5, (2260 + 74, 1272 + 74)));
        assert!(!hit(&r, STRIP_150, 1.5, (2260 + 75, 1272 + 74)));
    }

    #[test]
    fn hit_at_100_and_200_percent() {
        let r = [REGION];
        let s100 = rect(1720, 968, 1912, 1032);
        assert!(hit(&r, s100, 1.0, (1730, 988)));
        assert!(!hit(&r, s100, 1.0, (1770, 988)));
        let s200 = rect(3440, 1936, 3824, 2064);
        assert!(hit(&r, s200, 2.0, (3440 + 99, 1936 + 99)));
        assert!(!hit(&r, s200, 2.0, (3440 + 100, 1936 + 99)));
    }

    #[test]
    fn no_regions_or_outside_strip_never_hits() {
        assert!(!hit(&[], STRIP_150, 1.5, (2300, 1300)));
        assert!(!hit(&[REGION], STRIP_150, 1.5, (0, 0)));
    }

    #[test]
    fn any_of_several_regions_hits() {
        let second = Region { x: 100.0, ..REGION };
        let r = [REGION, second];
        assert!(hit(&r, STRIP_150, 1.5, (2260 + 160, 1272 + 40)));
        assert!(!hit(&r, STRIP_150, 1.5, (2260 + 120, 1272 + 40)));
    }
}
