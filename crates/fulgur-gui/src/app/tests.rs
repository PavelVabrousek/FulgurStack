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
                combine: Default::default(),
                ram: None,
            }),
            None,
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
fn mixed_bayer_headers_block_processing_until_the_owner_chooses() {
    let (mut app, id) = workspace_with_set("L", Some(HeaderLayout::Mixed));
    assert!(app.blocker().unwrap().contains("Choose colour or mono"));
    app.start(true);
    assert!(app.worker.is_none(), "no run without a declared layout");
    // Headers without any BAYERPAT are processed as mono without a choice.
    app.light_mut(id).unwrap().summary = Some(summary("L", Some(HeaderLayout::Missing)));
    assert!(app.blocker().is_none());
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
    // The intentionally starless fixture must reach registration and fail
    // explicitly. Under load the last render may already have consumed the
    // worker's stop event; then the app records the stop itself.
    match app.worker.as_ref() {
        Some(worker) => assert!(matches!(
            worker.events.recv_timeout(Duration::from_secs(10)).unwrap(),
            Event::Stopped {
                cancelled: false,
                ..
            }
        )),
        None => {
            assert!(matches!(app.last_run, Some(LastRun::Stopped)));
            assert!(
                app.message.starts_with("Unable to finish"),
                "{}",
                app.message
            );
        }
    }
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
        View::About,
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
        sky_scale: None,
        sky_held_out: None,
        sky_check: None,
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
fn quick_selections_keep_the_best_share_of_each_light_set() {
    use super::quality_table::best_selection;
    // Set 0: 20 frames, set 1: 10 frames; HFR grows with the index.
    let mut rows: Vec<QualityRow> = (0..30)
        .map(|i| {
            let mut r = row(i);
            r.set = usize::from(i >= 20);
            r.hfr = Some(1. + i as f64 / 10.);
            r
        })
        .collect();
    rows[3].can_include = false;
    let listed: Vec<usize> = (0..30).collect();
    let on_rows = |rows: &[QualityRow], fraction| -> Vec<String> {
        best_selection(rows, &listed, fraction)
            .into_iter()
            .filter(|(_, include)| *include)
            .map(|(p, _)| p.to_string_lossy().into_owned())
            .collect()
    };
    let on = |fraction| on_rows(&rows, fraction);
    // All: every usable frame, the unusable one is not switched on.
    let all = on(1.0);
    assert_eq!(all.len(), 29);
    assert!(!all.contains(&"frame_00003.fit".to_owned()));
    // 90%: ceil(19 * 0.9) = 18 of set 0 and 9 of set 1; the softest go off.
    let best = on(0.9);
    assert_eq!(best.len(), 27);
    assert!(!best.contains(&"frame_00019.fit".to_owned()));
    assert!(!best.contains(&"frame_00029.fit".to_owned()));
    assert!(best.contains(&"frame_00018.fit".to_owned()));
    // 95%: ceil(19 * 0.95) = 19 and ceil(10 * 0.95) = 10.
    assert_eq!(on(0.95).len(), 29);
    // 80%: ceil(15.2) = 16 + 8; 50%: ceil(9.5) = 10 + 5.
    assert_eq!(on(0.8).len(), 24);
    assert_eq!(on(0.5).len(), 15);
    assert_eq!(super::quality_table::QUICK_SELECTIONS.len(), 5);
    // Noise counts too: a sharp but noisy frame ranks last.
    let mut noisy = rows.clone();
    noisy[0].noise = Some(100.);
    assert!(!on_rows(&noisy, 0.9).contains(&"frame_00000.fit".to_owned()));
    // Only listed frames are touched.
    assert_eq!(best_selection(&rows, &[1, 2], 0.5).len(), 2);
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
    // An empty folder adds nothing and changes nothing.
    std::fs::create_dir(dir.path().join("empty")).unwrap();
    app.add_light_sources(vec![dir.path().join("empty")], None);
    assert_eq!(app.light_sets.len(), 1);
    assert!(
        app.message.contains("No FITS or XISF files"),
        "{}",
        app.message
    );
    assert_eq!(app.rows.len(), 1, "nothing changed");
    // A folder with frames only in a sub-folder gives that sub-folder as its
    // own light set (ADR-140, ADR-159).
    std::fs::create_dir(dir.path().join("night2")).unwrap();
    std::fs::write(dir.path().join("night2").join("frame.fit"), b"x").unwrap();
    app.add_light_sources(vec![dir.path().to_path_buf()], None);
    assert_eq!(app.light_sets.len(), 2);
    assert_eq!(app.light_sets[1].name, "night2");
    assert!(app.rows.is_empty() && app.selected.is_none() && app.preview.is_none());
    assert!(matches!(app.selection, Selection::Light(_)));
    app.tile = (1024, 256);
    let config = app.request().config;
    assert_eq!((config.tile_width, config.tile_height), (1024, 256));
    assert_eq!(config.tile_mode, TileMode::Manual);
}

#[test]
fn a_calibration_set_filled_kind_by_kind_is_offered_and_a_raw_stack_is_flagged() {
    let (mut app, id) = workspace_with_set("IRCUT", Some(HeaderLayout::Missing));
    let calibration = app.new_id();
    app.calibration_sets.push(CalibrationSet {
        id: calibration,
        name: "Calibration set 1".into(),
        items: [None, None, None, None, None],
    });
    // An empty set is offered to nobody and flags nothing.
    app.offer_calibration(calibration);
    assert_eq!(app.light(id).unwrap().calibration, None);
    assert!(app.uncalibrated_with_offer().is_empty());
    // Darks added on their own, as "+ Empty" and a kind do it.
    app.calibration_mut(calibration).unwrap().items[CalibrationKind::Dark.index()] =
        Some(CalibrationItem {
            source: CalibrationSource::Frames(vec!["dark1.fit".into()]),
            enabled: true,
            summary: None,
            combine: Default::default(),
            ram: None,
        });
    assert_eq!(
        app.uncalibrated_with_offer(),
        [("Night 1".to_owned(), "Calibration set 1".to_owned())]
    );
    app.offer_calibration(calibration);
    assert_eq!(app.light(id).unwrap().calibration, Some(calibration));
    assert!(app.uncalibrated_with_offer().is_empty());
}

/// One frame of the window while `hovered` files are dragged over it or
/// `dropped` files land, as the window system hands them to the app.
fn file_frame(
    app: &mut Workspace,
    ctx: &egui::Context,
    hovered: &[PathBuf],
    dropped: &[PathBuf],
) -> egui::FullOutput {
    ctx.style_mut(|style| style.animation_time = 0.);
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280., 800.),
            )),
            hovered_files: hovered
                .iter()
                .map(|p| egui::HoveredFile {
                    path: Some(p.clone()),
                    ..Default::default()
                })
                .collect(),
            dropped_files: dropped
                .iter()
                .map(|p| egui::DroppedFile {
                    path: Some(p.clone()),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        },
        |ctx| app.draw(ctx),
    )
}

fn frames_in(folder: &Path, count: usize) {
    std::fs::create_dir_all(folder).unwrap();
    for i in 0..count {
        std::fs::write(folder.join(format!("light_{i}.fit")), b"SIMPLE  =").unwrap();
    }
}

#[test]
fn folders_dropped_on_the_start_screen_become_light_sets_or_say_why_not() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let hint = |output: &egui::FullOutput, text: &str| {
        texts(output).iter().any(|(_, t)| t.starts_with(text))
    };
    // A folder with frames of its own: one light set.
    let night = dir.path().join("M 45_sub");
    frames_in(&night, 2);
    let mut app = Workspace::default();
    let hover = file_frame(&mut app, &ctx, std::slice::from_ref(&night), &[]);
    assert!(hint(&hover, "New light set"));
    file_frame(&mut app, &ctx, &[], std::slice::from_ref(&night));
    assert_eq!(app.light_sets.len(), 1, "{}", app.message);
    assert_eq!(app.light_sets[0].name, "M 45_sub");
    // A target folder with one folder per filter and a flat folder: one set
    // per filter, the flats left out and named.
    let target = dir.path().join("20261105 Olyhen");
    for filter in ["Olyhen Ha", "Olyhen L", "Olyhen OIII"] {
        frames_in(&target.join(filter), 2);
    }
    frames_in(&target.join("Flat L 0.5s"), 2);
    let mut app = Workspace::default();
    let hover = file_frame(&mut app, &ctx, std::slice::from_ref(&target), &[]);
    assert!(hint(
        &hover,
        "3 new light sets, one per folder · 1 calibration folder left out"
    ));
    file_frame(&mut app, &ctx, &[], std::slice::from_ref(&target));
    let names: Vec<_> = app.light_sets.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Olyhen Ha", "Olyhen L", "Olyhen OIII"]);
    assert!(app.message.contains("Flat L 0.5s"), "{}", app.message);
    // An empty folder: the hint says so before the drop, the start screen
    // after it, and no set appears.
    let empty = dir.path().join("olyhen RGB");
    std::fs::create_dir(&empty).unwrap();
    let mut app = Workspace::default();
    let hover = file_frame(&mut app, &ctx, std::slice::from_ref(&empty), &[]);
    assert!(hint(&hover, "No FITS or XISF files here"));
    file_frame(&mut app, &ctx, &[], std::slice::from_ref(&empty));
    assert!(app.light_sets.is_empty());
    let shown = file_frame(&mut app, &ctx, &[], &[]);
    assert!(hint(&shown, "⚠ No FITS or XISF files in olyhen RGB"));
}

fn camera_frames_in(folder: &Path, camera: &str, pattern: Option<&str>, count: usize) {
    std::fs::create_dir_all(folder).unwrap();
    for i in 0..count {
        let mut cards = vec![
            "SIMPLE  =                    T".to_owned(),
            "BITPIX  =                   16".to_owned(),
            "NAXIS   =                    2".to_owned(),
            "NAXIS1  =                    8".to_owned(),
            "NAXIS2  =                    8".to_owned(),
            "BZERO   =                32768".to_owned(),
            "EXPTIME =                 300.".to_owned(),
            format!("INSTRUME= '{camera:<8}'"),
        ];
        if let Some(pattern) = pattern {
            cards.push(format!("BAYERPAT= '{pattern:<8}'"));
        }
        cards.push("END".to_owned());
        let mut bytes = Vec::new();
        for card in cards {
            bytes.extend(format!("{card:<80}").bytes());
        }
        bytes.resize(2880, b' ');
        bytes.resize(5760, 0);
        std::fs::write(folder.join(format!("light_{i}.fit")), bytes).unwrap();
    }
}

#[test]
fn a_folder_of_another_camera_dropped_on_a_set_becomes_its_own_set() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let mono = dir.path().join("ASI2600MM L");
    let colour = dir.path().join("ASI2600MC nofilter");
    let more_mono = dir.path().join("ASI2600MM L night 2");
    camera_frames_in(&mono, "ZWO ASI2600MM Air", None, 2);
    camera_frames_in(&colour, "ZWO ASI2600MC Air", Some("RGGB"), 2);
    camera_frames_in(&more_mono, "ZWO ASI2600MM Air", None, 2);
    let mut app = Workspace::default();
    app.add_light_sources(vec![mono], Some(&ctx));
    let first = app.light_sets[0].id;
    // The same camera extends the set it was dropped on.
    app.take_drop(
        Some(file_drop::DropTarget::Light(first)),
        vec![more_mono],
        &ctx,
    );
    assert_eq!(app.light_sets.len(), 1, "{}", app.message);
    assert_eq!(app.light_sets[0].sources.len(), 2);
    // Another camera (header camera and Bayer pattern) never does.
    app.take_drop(
        Some(file_drop::DropTarget::Light(first)),
        vec![colour.clone()],
        &ctx,
    );
    assert_eq!(app.light_sets.len(), 2, "{}", app.message);
    assert_eq!(app.light_sets[0].sources.len(), 2);
    assert_eq!(app.light_sets[1].sources, [colour]);
    assert!(
        app.message.contains("light set of its own"),
        "{}",
        app.message
    );
}

#[test]
fn light_sets_of_other_cameras_switch_registration_to_plate_solving_once() {
    let set = |id: u64, camera: &str, pattern: Option<fulgur_core::BayerPattern>| LightSet {
        id,
        name: camera.into(),
        sources: vec![PathBuf::from(camera)],
        layout: LayoutChoice::Auto,
        exposure: None,
        derived_confirmed: false,
        include: true,
        calibration: None,
        summary: Some(sets::SetSummary {
            files: 3,
            width: 6248,
            height: 4176,
            cameras: vec![camera.into()],
            header_layout: Some(match pattern {
                Some(p) => sets::HeaderLayout::Bayer(p),
                None => sets::HeaderLayout::Missing,
            }),
            ..Default::default()
        }),
    };
    let mut app = Workspace::default();
    app.sky.download = true;
    app.light_sets = vec![set(1, "ASI2600MM", None)];
    assert_eq!(app.mixed_light_sets(), None, "one set is never mixed");
    app.light_sets
        .push(set(2, "ASI2600MC", Some(fulgur_core::BayerPattern::RGGB)));
    assert_eq!(
        app.mixed_light_sets().as_deref(),
        Some("cameras, colour layouts")
    );
    app.auto_registration_method();
    assert!(app.sky.enabled, "{}", app.message);
    // The owner's own choice of Stars is respected for the session.
    app.sky.enabled = false;
    app.sky.method_chosen = true;
    app.auto_registration_method();
    assert!(!app.sky.enabled);
    // Without a Gaia catalogue the method stays on Stars.
    let mut app = Workspace {
        light_sets: vec![set(1, "A", None), set(2, "B", None)],
        ..Workspace::default()
    };
    app.auto_registration_method();
    assert!(!app.sky.enabled);
    // Same camera and layout: not mixed.
    app.light_sets = vec![set(1, "A", None), set(2, "A", None)];
    assert_eq!(app.mixed_light_sets(), None);
}
