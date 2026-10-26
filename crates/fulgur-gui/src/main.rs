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
    let inputs = if smoke {
        vec![std::path::PathBuf::from(&args[2])]
    } else {
        args.iter().map(std::path::PathBuf::from).collect()
    };
    eframe::run_native(
        "FulgurStack",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_icon(icon)
                .with_inner_size([1060., 520.])
                .with_min_inner_size([800., 460.])
                .with_maximized(false)
                .with_drag_and_drop(true),
            centered: true,
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut app = if smoke {
                fulgur_gui::app::Workspace::new_transient(cc, inputs)
            } else {
                fulgur_gui::app::Workspace::new(cc, inputs)
            };
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
