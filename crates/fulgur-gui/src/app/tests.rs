use super::*;
use crate::sets::{HeaderLayout, SetSummary};
use fulgur_pipeline::register::ReferenceStrategy;

fn summary(filter: &str, layout: Option<HeaderLayout>) -> SetSummary {
    SetSummary {
        files: 3,
        width: 64,
        height: 64,
        header_layout: layout,
        exposures: vec![10.],
        filters: vec![filter.into()],
        cameras: vec!["Seestar S50 Pro".into()],
        ..SetSummary::default()
    }
}

fn workspace_with_set(filter: &str, layout: Option<HeaderLayout>) -> (Workspace, u64) {
    let mut app = Workspace::default();
    let id = app.push_light_set("Night 1".into(), vec!["night1".into()]);
    app.light_mut(id).unwrap().summary = Some(summary(filter, layout));
    (app, id)
}

fn render(app: &mut Workspace, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.style_mut(|style| style.animation_time = 0.);
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280., 800.),
            )),
            events,
            ..Default::default()
        },
        |ctx| app.draw(ctx),
    )
}

fn texts(output: &egui::FullOutput) -> Vec<(egui::Pos2, String)> {
    output
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::epaint::Shape::Text(t) = &s.shape {
                Some((t.pos, t.galley.job.text.clone()))
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn restored_reference_strategy_cannot_be_overridden_by_a_stale_reference_path() {
    for (mode, strategy, explicit) in [
        (0, ReferenceStrategy::Auto, false),
        (1, ReferenceStrategy::Auto, true),
        (2, ReferenceStrategy::Auto, true),
        (3, ReferenceStrategy::ChronologicalMedian, false),
    ] {
        let saved = AppSettings {
            reference_mode: mode,
            reference_path: "previous-reference.fit".into(),
            ..AppSettings::default()
        };
        let saved = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        let mut app = Workspace::default();
        app.restore_settings(saved);
        let request = app.request();
        assert_eq!(request.reference_strategy, strategy);
        assert_eq!(request.reference.is_some(), explicit);
    }
}

#[test]
fn light_and_calibration_sets_survive_settings_and_old_input_lists_migrate() {
    let (mut app, id) = workspace_with_set("IRCUT", Some(HeaderLayout::Missing));
    app.set_layout(id, LayoutChoice::Mono);
    let calibration = app.new_id();
    app.calibration_sets.push(CalibrationSet {
        id: calibration,
        name: "Flats 6 Oct".into(),
        items: [
            None,
            None,
            Some(CalibrationItem {
                source: CalibrationSource::Frames(vec!["flat1.fit".into(), "flat2.fit".into()]),
                enabled: true,
                summary: None,
            }),
            None,
        ],
    });
    app.light_mut(id).unwrap().calibration = Some(calibration);
    app.light_mut(id).unwrap().include = false;
    let json = serde_json::to_string(&app.settings_snapshot()).unwrap();
    let mut restored = Workspace::default();
    restored.restore_settings(serde_json::from_str(&json).unwrap());
    let set = &restored.light_sets[0];
    assert_eq!(
        (set.name.as_str(), set.layout, set.include),
        ("Night 1", LayoutChoice::Mono, false)
    );
    let cal = restored.calibration(set.calibration.unwrap()).unwrap();
    assert_eq!(cal.name, "Flats 6 Oct");
    assert_eq!(cal.item(CalibrationKind::Flat).unwrap().count(), 2);

    let old: AppSettings =
        serde_json::from_str(r#"{"schema_version":1,"inputs":["C:/A/M33 Subs","C:/B/M33 LP"]}"#)
            .unwrap();
    let mut migrated = Workspace::default();
    migrated.restore_settings(old);
    let names: Vec<_> = migrated
        .light_sets
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["M33 Subs", "M33 LP"]);
    assert!(
        migrated
            .light_sets
            .iter()
            .all(|s| s.layout == LayoutChoice::Auto && s.include)
    );
}

#[test]
fn unknown_bayer_layout_blocks_processing_until_the_owner_chooses() {
    let (mut app, id) = workspace_with_set("L", Some(HeaderLayout::Missing));
    assert!(app.blocker().unwrap().contains("Choose colour or mono"));
    app.start(true);
    assert!(app.worker.is_none(), "no run without a declared layout");
    app.set_layout(id, LayoutChoice::Mono);
    assert!(app.blocker().is_none());
    let request = app.request();
    assert_eq!(request.sets.len(), 1);
    assert_eq!(request.sets[0].layout, LayoutChoice::Mono);
    assert_eq!(request.provenance[0]["layout"], "mono");
    assert_eq!(request.provenance[0]["calibration_applied"], false);
    app.set_include(id, false);
    assert!(app.blocker().unwrap().contains("Include at least one"));
    assert!(app.request().sets.is_empty());
}

#[test]
fn mixed_filters_ask_before_stacking_and_set_changes_clear_measurements() {
    let (mut app, _) = workspace_with_set(
        "IRCUT",
        Some(HeaderLayout::Bayer(fulgur_core::BayerPattern::GRBG)),
    );
    let lp = app.push_light_set("LP".into(), vec!["lp".into()]);
    app.light_mut(lp).unwrap().summary = Some(summary(
        "LP",
        Some(HeaderLayout::Bayer(fulgur_core::BayerPattern::GRBG)),
    ));
    assert!(app.mixed_filters());
    app.rows = vec![row(0)];
    app.request_stack();
    assert_eq!(app.menu, Some(Menu::ConfirmMixed));
    assert!(app.worker.is_none(), "the confirmation comes first");
    app.set_include(lp, false);
    assert!(!app.mixed_filters());
    assert!(
        app.rows.is_empty(),
        "membership changes invalidate measurements"
    );
}

#[test]
fn review_is_static_and_top_bar_continue_button_really_resumes_worker() {
    let dir = tempfile::tempdir().unwrap();
    let request = crate::job::tests::request(dir.path());
    let worker = Worker::start(request, false);
    let Event::Quality {
        rows,
        waiting: true,
        ..
    } = worker.events.recv_timeout(Duration::from_secs(10)).unwrap()
    else {
        panic!("Expected review");
    };
    let now = Instant::now();
    let (mut app, _) = workspace_with_set(
        "IRCUT",
        Some(HeaderLayout::Bayer(fulgur_core::BayerPattern::GRBG)),
    );
    app.worker = Some(worker);
    app.waiting = true;
    app.rows = rows;
    app.started = Some(now - Duration::from_secs(183));
    app.review_started = Some(now - Duration::from_secs(180));
    let ctx = egui::Context::default();
    let _ = render(&mut app, &ctx, vec![]);
    let output = render(&mut app, &ctx, vec![]);
    let texts = texts(&output);
    assert!(
        app.active_elapsed() < Duration::from_secs(4),
        "Review wait is not processing time"
    );
    let output = render(&mut app, &ctx, vec![]);
    assert!(output.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(1));
    let point = texts
        .iter()
        .find(|(_, s)| s == "Continue stacking")
        .map(|(p, _)| *p + egui::vec2(10., 8.))
        .expect("Continue is in the top bar while waiting");
    let button = |pressed| egui::Event::PointerButton {
        pos: point,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let _ = render(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(point), button(true)],
    );
    let _ = render(&mut app, &ctx, vec![button(false)]);
    assert!(!app.waiting);
    assert!(app.review_started.is_none());
    assert_eq!(app.view, View::Processing);
    // The intentionally starless fixture must reach registration and fail explicitly.
    assert!(matches!(
        app.worker
            .as_ref()
            .unwrap()
            .events
            .recv_timeout(Duration::from_secs(10))
            .unwrap(),
        Event::Stopped {
            cancelled: false,
            ..
        }
    ));
}

#[test]
fn idle_views_do_not_animate() {
    let (mut app, _) = workspace_with_set(
        "IRCUT",
        Some(HeaderLayout::Bayer(fulgur_core::BayerPattern::GRBG)),
    );
    for view in [
        View::Frames,
        View::Settings,
        View::Processing,
        View::Results,
    ] {
        app.view = view;
        let ctx = egui::Context::default();
        let _ = render(&mut app, &ctx, vec![]);
        let _ = render(&mut app, &ctx, vec![]);
        let output = render(&mut app, &ctx, vec![]);
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(1),
            "{view:?}"
        );
    }
}

fn row(i: usize) -> QualityRow {
    QualityRow {
        file_name: format!("frame_{i:05}.fit"),
        path: format!("frame_{i:05}.fit").into(),
        set: 0,
        set_name: "Night 1".into(),
        exposure: Some(10.),
        date: Some(format!("2026-10-06T22:{:02}:00", i % 60)),
        hfr: Some(1.5 + (i % 10) as f64 / 10.),
        stars: 100,
        eccentricity: Some(0.4),
        background: Some(1200.),
        noise: Some(0.8),
        weight: 0.7,
        accepted: i % 5 != 0,
        automatic_accepted: i % 5 != 0,
        can_include: true,
        reasons: if i % 5 == 0 {
            "tracking".into()
        } else {
            String::new()
        },
    }
}

#[test]
fn quality_virtualizes_large_sets_and_renders_all_views_at_windows_scales() {
    let ctx = egui::Context::default();
    let (mut app, id) = workspace_with_set(
        "IRCUT",
        Some(HeaderLayout::Bayer(fulgur_core::BayerPattern::GRBG)),
    );
    app.rows = (0..10000).map(row).collect();
    app.inspected_ids = vec![id];
    for scale in [1., 1.25, 1.5, 2.] {
        ctx.set_pixels_per_point(scale);
        for view in [
            View::Frames,
            View::Settings,
            View::Processing,
            View::Results,
        ] {
            app.view = view;
            for selection in [Selection::All, Selection::Light(id)] {
                app.selection = selection;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280., 800.),
                        )),
                        ..Default::default()
                    },
                    |ctx| app.draw(ctx),
                );
                if view == View::Frames {
                    let visible_rows = output.shapes.iter().filter(|s| matches!(&s.shape, egui::epaint::Shape::Text(t) if t.galley.job.text.starts_with("frame_"))).count();
                    assert!(
                        (1..100).contains(&visible_rows),
                        "Only visible table rows may be drawn: {visible_rows}"
                    );
                }
                assert!(!output.shapes.is_empty());
            }
        }
    }
    assert_eq!(app.rows.len(), 10000);
    assert!(app.worker.is_none());
    assert_eq!(app.request().config.fits_cache_mib, 0);
    assert!(app.request().reference.is_none());
}

#[test]
fn added_sets_discard_measurements_and_settings_keep_reference_grid() {
    let (mut app, _) = workspace_with_set(
        "IRCUT",
        Some(HeaderLayout::Bayer(fulgur_core::BayerPattern::GRBG)),
    );
    app.rows = vec![row(0)];
    app.inspection = None;
    app.selected = Some(0);
    let dir = tempfile::tempdir().unwrap();
    app.add_light_sources(vec![dir.path().to_path_buf()], None);
    assert_eq!(app.light_sets.len(), 2);
    assert!(app.rows.is_empty() && app.selected.is_none() && app.preview.is_none());
    assert!(matches!(app.selection, Selection::Light(_)));
    app.tile = 256;
    let config = app.request().config;
    assert_eq!((config.tile_width, config.tile_height), (256, 256));
    assert_eq!(config.tile_mode, TileMode::Manual);
}
