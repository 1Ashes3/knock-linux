//! Pure strip geometry (SPEC.md §9.2, §9.3; ADR 0002): which display, where
//! on it, how big, and the hit test. No OS calls; unit-tested on
//! recorded monitor data.

use crate::platform::{Display, Taskbar};
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

/// `Taskbar::edge` for the bottom of the screen (ABE_BOTTOM).
const BOTTOM: u32 = 3;

/// Where the strip is and what the cursor poll tests against (§9.3).
pub struct Strip {
    pub rect: Rect,
    pub scale: f64,
    pub regions: Vec<Region>,
    /// The size the UI last asked for, in logical px (`None`: not yet).
    pub want: Option<(f64, f64)>,
    /// The chosen monitor was missing at the last placement.
    pub fell_back: bool,
}

/// Physical rect of a strip `width` by `height` logical px whose bottom
/// edge sits at `corner.1` and whose right edge is 8 logical px left of
/// `corner.0`, so it grows leftward and upward from that corner.
pub fn place(corner: (i32, i32), scale: f64, width: f64, height: f64) -> Rect {
    let px = |logical: f64| (logical * scale).round() as i32;
    let (right, bottom) = (corner.0 - px(RIGHT_MARGIN), corner.1);
    Rect {
        left: right - px(width),
        top: bottom - px(height),
        right,
        bottom,
    }
}

/// Which display the §8.6 `monitor` setting means: the primary for
/// `"primary"`, else the one with that id. An id that isn't connected falls
/// back to the primary, with `true`. `None` only with no display at all.
pub fn choose<'a>(monitor: &str, displays: &'a [Display]) -> Option<(&'a Display, bool)> {
    let primary = displays.iter().find(|d| d.primary).or(displays.first());
    match displays
        .iter()
        .find(|d| monitor != "primary" && d.id == monitor)
    {
        Some(d) => Some((d, false)),
        None => primary.map(|d| (d, monitor != "primary")),
    }
}

/// The point the strip's bottom-right corner rests on (§9.2): the work
/// area's bottom-right, or with an auto-hidden bottom taskbar the top edge
/// of the shown taskbar (ADR 0002). A taskbar on another edge leaves the
/// work area's bottom-right. `primary_scale` is the primary display's.
pub fn corner(d: &Display, taskbar: Option<Taskbar>, primary_scale: f64) -> (i32, i32) {
    let wa = (d.work_area.right, d.work_area.bottom);
    let Some(t) = taskbar.filter(|t| t.auto_hide && t.edge == BOTTOM) else {
        return wa;
    };
    let top = match d.primary {
        true => t.rect.top,
        // ponytail: the shell reports only the primary taskbar. Another
        // display's taskbar is taken to be at its bottom with the same
        // logical height; read Shell_SecondaryTrayWnd if that proves wrong.
        false => {
            let logical = f64::from(t.rect.bottom - t.rect.top) / primary_scale;
            d.monitor.bottom - (logical * d.scale).round() as i32
        }
    };
    (wa.0, top.min(wa.1))
}

/// The result of [`layout`].
#[derive(Debug, PartialEq)]
pub struct Layout {
    pub rect: Rect,
    pub scale: f64,
    /// The chosen monitor is missing; this is the primary.
    pub fell_back: bool,
}

/// Where the strip goes (§9.2): on the display `monitor` names, `want`
/// logical px (default 3 slots) capped at 6 slots of `slot` px and the work
/// area's height, resting on [`corner`]. `None` with no display.
pub fn layout(
    monitor: &str,
    displays: &[Display],
    taskbar: Option<Taskbar>,
    want: Option<(f64, f64)>,
    slot: f64,
) -> Option<Layout> {
    let (d, fell_back) = choose(monitor, displays)?;
    let primary_scale = displays.iter().find(|d| d.primary).map_or(1.0, |p| p.scale);
    let (w, h) = want.unwrap_or((3.0 * slot, slot));
    let max_height = f64::from(d.work_area.bottom - d.work_area.top) / d.scale;
    let (w, h) = clamp(w, h, slot, max_height)?;
    Some(Layout {
        rect: place(corner(d, taskbar, primary_scale), d.scale, w, h),
        scale: d.scale,
        fell_back,
    })
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

    fn br(r: Rect) -> (i32, i32) {
        (r.right, r.bottom)
    }

    fn display(id: &str, primary: bool, monitor: Rect, work_area: Rect, scale: f64) -> Display {
        Display {
            id: id.into(),
            primary,
            monitor,
            work_area,
            scale,
        }
    }

    // Recorded on the user's machine (ADR 0002 evidence and Spike A, Windows
    // 11 25H2): 1920x1080 at 100%, bottom taskbar (0,1032)-(1920,1080).
    const MON: Rect = rect(0, 0, 1920, 1080);
    const BAR: Taskbar = Taskbar {
        rect: rect(0, 1032, 1920, 1080),
        edge: 3,
        auto_hide: false,
    };
    const HIDDEN: Taskbar = Taskbar {
        auto_hide: true,
        ..BAR
    };

    #[test]
    fn auto_hide_off_rests_on_the_work_area() {
        let d = [display("a", true, MON, rect(0, 0, 1920, 1032), 1.0)];
        let l = layout("primary", &d, Some(BAR), None, MEDIUM).unwrap();
        assert_eq!(l.rect, rect(1720, 968, 1912, 1032));
        assert!(!l.fell_back);
    }

    #[test]
    fn auto_hide_on_rests_on_the_shown_taskbar_not_the_screen_bottom() {
        // With auto-hide on, the work area is the whole monitor (ADR 0002).
        let d = [display("a", true, MON, MON, 1.0)];
        let l = layout("primary", &d, Some(HIDDEN), None, MEDIUM).unwrap();
        assert_eq!(l.rect, rect(1720, 968, 1912, 1032), "same as auto-hide off");
        // Without taskbar info it can only use the work area.
        let l = layout("primary", &d, None, None, MEDIUM).unwrap();
        assert_eq!(l.rect.bottom, 1080);
    }

    #[test]
    fn a_taskbar_on_another_edge_falls_back_to_the_work_area() {
        let wa = rect(48, 0, 1920, 1080);
        let left = Taskbar {
            rect: rect(0, 0, 48, 1080),
            edge: 0,
            auto_hide: true,
        };
        let d = display("a", true, MON, wa, 1.0);
        assert_eq!(corner(&d, Some(left), 1.0), (1920, 1080));
        let top = Taskbar { edge: 1, ..left };
        assert_eq!(corner(&d, Some(top), 1.0), (1920, 1080));
    }

    // Derived, not recorded (one monitor here): the primary above at 100%,
    // and a 3840x2160 display at 150% to its right.
    const SECOND: Rect = rect(1920, 0, 5760, 2160);

    #[test]
    fn two_displays_with_different_scale() {
        let d = [
            display("a", true, MON, rect(0, 0, 1920, 1032), 1.0),
            display("b", false, SECOND, rect(1920, 0, 5760, 2088), 1.5),
        ];
        let l = layout("b", &d, Some(BAR), None, MEDIUM).unwrap();
        // 64*1.5 = 96 tall, 192*1.5 = 288 wide, 8*1.5 = 12 margin.
        assert_eq!(l.rect, rect(5460, 1992, 5748, 2088));
        assert_eq!((l.scale, l.fell_back), (1.5, false));
        // Size setting large (80) at 150%: 120 physical, from logical px.
        let l = layout("b", &d, Some(BAR), Some((80.0, 80.0)), 80.0).unwrap();
        assert_eq!(l.rect, rect(5628, 1968, 5748, 2088));
        // "primary" still means the 100% one.
        let l = layout("primary", &d, Some(BAR), None, MEDIUM).unwrap();
        assert_eq!((l.rect, l.scale), (rect(1720, 968, 1912, 1032), 1.0));
    }

    #[test]
    fn two_displays_with_auto_hide_use_the_taskbars_logical_height() {
        let d = [
            display("a", true, MON, MON, 1.0),
            display("b", false, SECOND, SECOND, 1.5),
        ];
        // 48 logical px at 150% = 72: the shown taskbar's top is 2088.
        let l = layout("b", &d, Some(HIDDEN), None, MEDIUM).unwrap();
        assert_eq!(l.rect, rect(5460, 1992, 5748, 2088));
        // A primary at 200% with a 96 px taskbar, the other at 100%.
        let d = [
            display(
                "a",
                true,
                rect(0, 0, 3840, 2160),
                rect(0, 0, 3840, 2160),
                2.0,
            ),
            display(
                "b",
                false,
                rect(3840, 0, 5760, 1080),
                rect(3840, 0, 5760, 1080),
                1.0,
            ),
        ];
        let bar = Taskbar {
            rect: rect(0, 2064, 3840, 2160),
            ..HIDDEN
        };
        assert_eq!(corner(&d[1], Some(bar), 2.0), (5760, 1032));
        assert_eq!(corner(&d[0], Some(bar), 2.0), (3840, 2064));
    }

    #[test]
    fn a_missing_chosen_display_falls_back_to_the_primary() {
        let d = [
            display("b", false, SECOND, rect(1920, 0, 5760, 2088), 1.5),
            display("a", true, MON, rect(0, 0, 1920, 1032), 1.0),
        ];
        let l = layout("0123456789abcdef", &d, Some(BAR), None, MEDIUM).unwrap();
        assert_eq!((l.rect, l.fell_back), (rect(1720, 968, 1912, 1032), true));
        assert_eq!(choose("primary", &d).unwrap().0.id, "a");
        assert_eq!(
            choose("b", &d).map(|(d, f)| (d.id.as_str(), f)),
            Some(("b", false))
        );
        // An id equal to the word "primary" can't pick a non-primary display.
        let odd = [display("primary", false, SECOND, SECOND, 1.0), d[1].clone()];
        assert_eq!(choose("primary", &odd).unwrap().0.id, "a");
        // No primary flag (shouldn't happen): the first display.
        let none = [display("x", false, MON, MON, 1.0)];
        assert_eq!(choose("primary", &none).unwrap().0.id, "x");
        assert!(choose("primary", &[]).is_none());
        assert!(layout("primary", &[], None, None, MEDIUM).is_none());
    }

    #[test]
    fn layout_caps_the_requested_size() {
        let d = [display("a", true, MON, rect(0, 0, 1920, 1032), 1.0)];
        let l = layout("primary", &d, None, Some((1000.0, 64.0)), MEDIUM).unwrap();
        assert_eq!(l.rect, rect(1528, 968, 1912, 1032), "6 slots");
        let l = layout("primary", &d, None, Some((0.0, 64.0)), MEDIUM).unwrap();
        assert_eq!(l.rect.left, l.rect.right, "no sessions: zero width");
    }

    // 100%: recorded on the user's machine (1920x1080 monitor, bottom
    // taskbar 1032..1080, Windows 11 25H2). 150% and 200%: not recorded yet;
    // derived from a 48 logical px bottom taskbar (72 and 96 physical px).
    #[test]
    fn place_at_100_percent() {
        let wa = rect(0, 0, 1920, 1032);
        assert_eq!(
            place(br(wa), 1.0, THREE, MEDIUM),
            rect(1720, 968, 1912, 1032)
        );
    }

    #[test]
    fn place_at_150_percent() {
        let wa = rect(0, 0, 2560, 1368);
        // 64*1.5 = 96 tall, 192*1.5 = 288 wide, 8*1.5 = 12 margin.
        assert_eq!(
            place(br(wa), 1.5, THREE, MEDIUM),
            rect(2260, 1272, 2548, 1368)
        );
    }

    #[test]
    fn place_at_200_percent() {
        let wa = rect(0, 0, 3840, 2064);
        assert_eq!(
            place(br(wa), 2.0, THREE, MEDIUM),
            rect(3440, 1936, 3824, 2064)
        );
    }

    #[test]
    fn place_on_offset_monitor_rounds_fractional_scale() {
        // Primary monitor need not start at 0,0; 125% gives fractional px.
        let wa = rect(-1920, 200, 0, 1232);
        // 64*1.25 = 80, 192*1.25 = 240, 8*1.25 = 10.
        assert_eq!(
            place(br(wa), 1.25, THREE, MEDIUM),
            rect(-250, 1152, -10, 1232)
        );
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
        assert_eq!(
            place(br(wa), 1.5, 384.0, 64.0),
            rect(1972, 1272, 2548, 1368)
        );
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
