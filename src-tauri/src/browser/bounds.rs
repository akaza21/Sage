use tauri::{LogicalPosition, LogicalSize, Manager, State};

use super::BrowserState;

/// The smallest step the platform can place a child webview by.
#[derive(Clone, Copy, Debug, PartialEq)]
enum PlacementGrid {
    /// One device pixel: WebView2 places in physical pixels, and AppKit
    /// frames take fractional points.
    DevicePixel,
    /// One logical pixel: GTK 3 allocations are whole logical pixels.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    LogicalPixel,
}

#[cfg(target_os = "linux")]
const PLATFORM_GRID: PlacementGrid = PlacementGrid::LogicalPixel;
#[cfg(not(target_os = "linux"))]
const PLATFORM_GRID: PlacementGrid = PlacementGrid::DevicePixel;

/// Absorbs float error in CSS rectangles that already sit on the grid.
const GRID_TOLERANCE: f64 = 1e-6;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Frame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Snaps a CSS-pixel frame inward to `grid` at `scale`: left and top round up,
/// right and bottom round down. The page then never covers the chrome around
/// it, and leaves less than one grid unit of the frame uncovered per edge.
/// `None` when nothing of the frame is left to show.
fn snap_inward(frame: Frame, scale: f64, grid: PlacementGrid) -> Option<Frame> {
    let units_per_logical = match grid {
        PlacementGrid::DevicePixel => scale,
        PlacementGrid::LogicalPixel => 1.0,
    };
    let up = |value: f64| (value * units_per_logical - GRID_TOLERANCE).ceil();
    let down = |value: f64| (value * units_per_logical + GRID_TOLERANCE).floor();
    let (left, top) = (up(frame.x), up(frame.y));
    let (right, bottom) = (down(frame.x + frame.width), down(frame.y + frame.height));
    (right > left && bottom > top).then(|| Frame {
        x: left / units_per_logical,
        y: top / units_per_logical,
        width: (right - left) / units_per_logical,
        height: (bottom - top) / units_per_logical,
    })
}

#[tauri::command]
pub(crate) async fn browser_set_bounds(
    state: State<'_, BrowserState>,
    tab_id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    if ![x, y, width, height].iter().all(|value| value.is_finite())
        || x < 0.0
        || y < 0.0
        || width < 1.0
        || height < 1.0
        || width > 10000.0
        || height > 10000.0
    {
        return Err("invalid browser bounds".into());
    }
    let tabs = state.tabs.lock().map_err(|error| error.to_string())?;
    let tab = tabs.entries.get(&tab_id).ok_or("browser tab unavailable")?;
    let webview = &tab.handle;
    let visibility = tab.visibility.clone();
    let scale = webview
        .window()
        .scale_factor()
        .map_err(|error| error.to_string())?;
    let frame = snap_inward(
        Frame {
            x,
            y,
            width,
            height,
        },
        scale,
        PLATFORM_GRID,
    )
    .ok_or("invalid browser bounds")?;
    webview
        .set_position(LogicalPosition::new(frame.x, frame.y))
        .and_then(|()| webview.set_size(LogicalSize::new(frame.width, frame.height)))
        .map_err(|error| error.to_string())?;
    // A page frozen for an Annotate comment stays hidden until the comment
    // is done. The check runs on the main thread, where Annotate's hide runs
    // too and only after it marks the page frozen, so a show checked before
    // the mark is always followed by that hide.
    let app = webview.app_handle().clone();
    let shown = webview.clone();
    app.clone()
        .run_on_main_thread(move || {
            visibility.place();
            // Placed again, the page is no longer covered by a popover.
            if app.state::<BrowserState>().annotate.placed(&tab_id)
                && let Err(error) = shown.show()
            {
                eprintln!("browser show failed for {tab_id}: {error}");
            }
        })
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{Frame, PlacementGrid, snap_inward};

    fn frame(x: f64, y: f64, width: f64, height: f64) -> Frame {
        Frame {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_frame_on_the_grid_is_unchanged_at_every_scale() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let on_grid = frame(8.0, 40.0, 320.0, 200.0);
            assert_eq!(
                snap_inward(on_grid, scale, PlacementGrid::DevicePixel),
                Some(on_grid),
                "scale {scale}"
            );
            assert_eq!(
                snap_inward(on_grid, scale, PlacementGrid::LogicalPixel),
                Some(on_grid),
                "scale {scale}"
            );
        }
    }

    #[test]
    fn fractional_edges_snap_inward_to_device_pixels() {
        // At 1.25, device pixels are 0.8 logical pixels apart.
        let snapped = snap_inward(
            frame(10.1, 20.3, 100.0, 50.0),
            1.25,
            PlacementGrid::DevicePixel,
        )
        .unwrap();
        let device_edges = [
            snapped.x,
            snapped.y,
            snapped.x + snapped.width,
            snapped.y + snapped.height,
        ]
        .map(|edge| edge * 1.25);
        for (edge, expected) in device_edges.into_iter().zip([13.0, 26.0, 137.0, 87.0]) {
            assert!((edge - expected).abs() < 1e-9, "{edge} != {expected}");
        }
    }

    #[test]
    fn the_odd_device_pixel_at_scale_two_is_left_uncovered_on_the_logical_grid() {
        // 201 device pixels at scale 2 is 100.5 logical pixels, which GTK
        // cannot place: the page takes 100 and leaves half a pixel uncovered.
        let snapped = snap_inward(
            frame(0.0, 0.0, 100.5, 80.0),
            2.0,
            PlacementGrid::LogicalPixel,
        )
        .unwrap();
        assert_eq!(snapped, frame(0.0, 0.0, 100.0, 80.0));
        let snapped = snap_inward(
            frame(0.0, 0.0, 100.5, 80.0),
            2.0,
            PlacementGrid::DevicePixel,
        )
        .unwrap();
        assert_eq!(snapped, frame(0.0, 0.0, 100.5, 80.0));
    }

    #[test]
    fn the_page_never_crosses_the_frame_and_leaves_less_than_one_unit_per_edge() {
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0, 3.0] {
            for grid in [PlacementGrid::DevicePixel, PlacementGrid::LogicalPixel] {
                let unit = match grid {
                    PlacementGrid::DevicePixel => 1.0 / scale,
                    PlacementGrid::LogicalPixel => 1.0,
                };
                for step in 0..40 {
                    let offset = f64::from(step) * 0.137;
                    let css = frame(30.0 + offset, 48.0 + offset, 400.3 - offset, 260.7);
                    let snapped = snap_inward(css, scale, grid).unwrap();
                    let gaps = [
                        snapped.x - css.x,
                        snapped.y - css.y,
                        (css.x + css.width) - (snapped.x + snapped.width),
                        (css.y + css.height) - (snapped.y + snapped.height),
                    ];
                    for gap in gaps {
                        assert!(
                            (-1e-9..unit).contains(&gap),
                            "{grid:?} scale {scale} css {css:?} gap {gap}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn neighbouring_frames_never_overlap() {
        // The toolbar ends at 41.3 and the page frame starts there.
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for grid in [PlacementGrid::DevicePixel, PlacementGrid::LogicalPixel] {
                let page = snap_inward(frame(0.0, 41.3, 500.0, 300.0), scale, grid).unwrap();
                assert!(page.y >= 41.3, "{grid:?} scale {scale}");
            }
        }
    }

    #[test]
    fn a_frame_thinner_than_one_unit_snaps_to_nothing() {
        assert_eq!(
            snap_inward(
                frame(10.2, 10.0, 1.0, 50.0),
                1.0,
                PlacementGrid::LogicalPixel
            ),
            None
        );
    }
}
