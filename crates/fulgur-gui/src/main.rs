#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result<()> {
    #[cfg(windows)]
    {
        let identity: Vec<u16> = "FulgurStack.Desktop\0".encode_utf16().collect();
        // SAFETY: identity is a live, null-terminated UTF-16 application identifier.
        let result = unsafe {
            windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(
                identity.as_ptr(),
            )
        };
        if result < 0 {
            eprintln!("Windows application identity could not be set: {result:#x}");
        }
    }
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/fulgurstack.png"))
        .expect("The embedded FulgurStack application icon must be a valid PNG");
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let quick = args.first().is_some_and(|s| s == "--ui-smoke-quick");
    let smoke = quick || args.first().is_some_and(|s| s == "--ui-smoke");
    if smoke && args.len() != 4 {
        eprintln!(
            "Usage: fulgurstack-gui --ui-smoke <existing-output-folder> <input-folder> <reference-fits>"
        );
        return Ok(());
    }
    // --ui-guide <output-folder> <prefix> <measure|stack> <lights|saved> [calibration...]
    let guide = args.first().is_some_and(|s| s == "--ui-guide");
    if guide && args.len() < 5 {
        eprintln!(
            "Usage: fulgurstack-gui --ui-guide <output-folder> <prefix> <measure|stack> <lights|saved> [calibration...]"
        );
        return Ok(());
    }
    let saved = guide && args[4] == "saved";
    let inputs = if smoke {
        vec![std::path::PathBuf::from(&args[2])]
    } else if saved {
        vec![]
    } else if guide {
        vec![std::path::PathBuf::from(&args[4])]
    } else {
        // Unknown options are not files to open.
        args.iter()
            .filter(|a| !a.to_string_lossy().starts_with("--"))
            .map(std::path::PathBuf::from)
            .collect()
    };
    let window = if smoke || guide {
        None
    } else {
        fulgur_gui::app::saved_window()
    };
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title(format!("FulgurStack {}", fulgur_gui::VERSION))
        .with_icon(icon)
        .with_inner_size(window.map_or([1060., 520.], |w| [w.width, w.height]))
        .with_min_inner_size([800., 460.])
        .with_maximized(window.is_some_and(|w| w.maximized))
        .with_drag_and_drop(true);
    let place = window.filter(|w| w.on_screen());
    if let Some(w) = place {
        viewport = viewport.with_position([w.x, w.y]);
    }
    eframe::run_native(
        "FulgurStack",
        eframe::NativeOptions {
            viewport,
            centered: place.is_none(),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut app = if smoke || guide {
                fulgur_gui::app::Workspace::new_transient(cc, inputs)
            } else {
                fulgur_gui::app::Workspace::new(cc, inputs)
            };
            if guide {
                app.enable_guide(fulgur_gui::app::GuideTour::new(
                    std::path::PathBuf::from(&args[1]),
                    args[2].to_string_lossy().into_owned(),
                    args[3] == "stack",
                    args.iter().skip(5).map(std::path::PathBuf::from).collect(),
                    saved,
                ));
            }
            if smoke {
                let folder = std::path::PathBuf::from(&args[1]);
                let reference = std::path::PathBuf::from(&args[3]);
                if quick {
                    app.enable_quick_smoke(folder, reference);
                } else {
                    app.enable_smoke(folder, reference);
                }
            }
            Ok(Box::new(app))
        }),
    )
}
