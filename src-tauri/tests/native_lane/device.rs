use std::{io::Cursor, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use sage_lib::native_lane::Lane;
use serde_json::json;

use crate::{
    report::{Report, wait_for},
    site::Site,
};

pub fn run(lane: &Lane, site: &Site, report: &mut Report) {
    report.section("Device DPR changes resolution without magnifying the native view");
    let result = check_devices(lane, site, report);
    report.check("device.completed", result.is_ok(), format!("{result:?}"));
}

fn check_devices(lane: &Lane, site: &Site, report: &mut Report) -> Result<(), String> {
    lane.show_device_window()?;
    let url = site.url("/device.html");
    let tab = lane.open_tab(Some(&url), false)?;
    if wait_for(Duration::from_secs(15), || {
        lane.settled_url(&tab).filter(|settled| settled == &url)
    })
    .as_deref()
        != Some(&url)
    {
        return Err("device fixture did not settle".into());
    }
    let engine_dpr = lane
        .evaluate(&tab, "devicePixelRatio", false)?
        .as_f64()
        .ok_or("missing engine DPR")?;
    for (preset, width, height, dpr) in [
        ("responsive", 500, 700, engine_dpr),
        ("iphone-se", 375, 667, 2.0),
        ("pixel-7", 412, 915, 2.625),
    ] {
        if preset != "responsive" {
            // A narrow desktop layout with text can leave a non-default page
            // scale behind when Chromium switches to the mobile viewport.
            narrow_desktop(lane, &tab)?;
        }
        for percent in [100, 50] {
            lane.place_device(
                &tab,
                f64::from(width * percent) / 100.0,
                f64::from(height * percent) / 100.0,
            )?;
            lane.set_device(
                &tab,
                Some(json!({"preset":preset,"width":width,"height":height,"scalePercent":percent})),
            )?;
            let metrics = lane.evaluate(
                &tab,
                "[innerWidth, innerHeight, devicePixelRatio, screen.width, screen.height]",
                false,
            )?;
            report.check(
                &format!("device.{preset}.{percent}.metrics"),
                metrics[0] == width
                    && metrics[1] == height
                    && metrics[2].as_f64() == Some(dpr)
                    && metrics[3] == width
                    && metrics[4] == height,
                metrics.to_string(),
            );
            // Let the compositor present the new metrics before reading pixels.
            let corners = wait_for(Duration::from_secs(5), || {
                let pixels = preview_corners(lane, &tab).ok()?;
                (pixels == [[0, 0, 255], [255, 0, 0], [255, 255, 0]]).then_some(pixels)
            });
            report.check(
                &format!("device.{preset}.{percent}.corners"),
                corners.is_some(),
                format!("corners={:?}", preview_corners(lane, &tab)),
            );
        }
    }
    let plain = site.url("/plain.html");
    lane.navigate(&tab, &plain)?;
    if wait_for(Duration::from_secs(15), || {
        lane.settled_url(&tab).filter(|settled| settled == &plain)
    })
    .as_deref()
        != Some(&plain)
    {
        return Err("page without viewport metadata did not settle".into());
    }
    let navigated = lane.evaluate(
        &tab,
        "[innerWidth, visualViewport.width, devicePixelRatio]",
        false,
    )?;
    report.check(
        "device.navigation.no-meta",
        fitted_wide_layout(&navigated, 2.625),
        navigated.to_string(),
    );
    lane.set_device(&tab, None)?;
    narrow_desktop(lane, &tab)?;
    lane.place_device(&tab, 375.0, 667.0)?;
    lane.set_device(
        &tab,
        Some(json!({"preset":"iphone-se","width":375,"height":667,"scalePercent":100})),
    )?;
    let plain_metrics = lane.evaluate(
        &tab,
        "[innerWidth, visualViewport.width, devicePixelRatio]",
        false,
    )?;
    report.check(
        "device.no-viewport-meta",
        fitted_wide_layout(&plain_metrics, 2.0),
        plain_metrics.to_string(),
    );
    lane.navigate(&tab, &url)?;
    if wait_for(Duration::from_secs(15), || {
        lane.settled_url(&tab).filter(|settled| settled == &url)
    })
    .as_deref()
        != Some(&url)
    {
        return Err("viewport fixture did not return".into());
    }
    let returned = wait_for(Duration::from_secs(5), || {
        (preview_corners(lane, &tab).ok()? == [[0, 0, 255], [255, 0, 0], [255, 255, 0]])
            .then_some(())
    });
    report.check(
        "device.navigation.viewport-meta",
        returned.is_some(),
        format!("corners={:?}", preview_corners(lane, &tab)),
    );
    lane.set_device(&tab, None)?;
    lane.evaluate(&tab, "document.querySelector('meta[name=viewport]').content = 'width=device-width,initial-scale=2'", false)?;
    narrow_desktop(lane, &tab)?;
    lane.place_device(&tab, 375.0, 667.0)?;
    lane.set_device(
        &tab,
        Some(json!({"preset":"iphone-se","width":375,"height":667,"scalePercent":100})),
    )?;
    let authored = lane.evaluate(&tab, "visualViewport.scale", false)?;
    report.check(
        "device.authored-scale",
        authored.as_f64() == Some(2.0),
        authored.to_string(),
    );
    lane.cdp(
        &tab,
        "Emulation.setPageScaleFactor",
        &json!({"pageScaleFactor":1.5}),
    )?;
    lane.place_device(&tab, 187.5, 333.5)?;
    lane.set_device(
        &tab,
        Some(json!({"preset":"iphone-se","width":375,"height":667,"scalePercent":50})),
    )?;
    let pinch = lane.evaluate(&tab, "visualViewport.scale", false)?;
    report.check(
        "device.fit-preserves-page-scale",
        pinch.as_f64() == Some(1.5),
        pinch.to_string(),
    );
    lane.set_device(&tab, None)?;
    Ok(())
}

fn narrow_desktop(lane: &Lane, tab: &str) -> Result<(), String> {
    lane.place_device(tab, 50.0, 700.0)?;
    lane.set_device(
        tab,
        Some(json!({"preset":"responsive","width":50,"height":700,"scalePercent":100})),
    )?;
    // Force the narrow document to lay out before switching its viewport mode.
    lane.evaluate(tab, "document.body.offsetWidth", false)?;
    Ok(())
}

fn fitted_wide_layout(metrics: &serde_json::Value, dpr: f64) -> bool {
    let layout = metrics[0].as_f64().unwrap_or_default();
    let visible = metrics[1].as_f64().unwrap_or_default();
    layout >= 980.0 && (layout - visible).abs() <= 1.0 && metrics[2].as_f64() == Some(dpr)
}

fn preview_corners(lane: &Lane, tab: &str) -> Result<[[u8; 3]; 3], String> {
    // On WebView2 this is the actual native surface, with its existing bounds.
    // Do not pass a clip or capture beyond it: either can conceal magnification.
    let shot = lane.cdp(
        tab,
        "Page.captureScreenshot",
        &json!({
            "format": "png", "fromSurface": true, "captureBeyondViewport": false,
        }),
    )?;
    let bytes = STANDARD
        .decode(shot["data"].as_str().ok_or("missing preview")?)
        .map_err(|error| error.to_string())?;
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    let mut data = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut data)
        .map_err(|error| error.to_string())?;
    let channels = info.color_type.samples();
    if channels < 3 || info.width < 8 || info.height < 8 {
        return Err("invalid preview".into());
    }
    let pixel = |x: u32, y: u32| {
        let offset = (y as usize * info.width as usize + x as usize) * channels;
        [data[offset], data[offset + 1], data[offset + 2]]
    };
    Ok([
        pixel(info.width - 4, 4),
        pixel(info.width - 4, info.height - 4),
        pixel(4, info.height - 4),
    ])
}
