//! Sky registration options: where the Gaia stars come from and where the
//! field is.
use fulgur_core::{Error, Result};
use std::path::PathBuf;

/// Sky registration inputs: where the Gaia stars come from and where the
/// field is.
#[derive(clap::Args, Debug, Clone)]
pub(crate) struct SkyArgs {
    /// Folder of the PixInsight Gaia DR3 .xpsd files (check it with gaia-check).
    #[arg(long)]
    gaia_db: Option<PathBuf>,
    /// Folder for downloaded Gaia cones (default: the system temporary folder).
    #[arg(long)]
    gaia_cache: Option<PathBuf>,
    /// Allow downloading Gaia stars the local files and cache do not hold.
    #[arg(long)]
    gaia_download: bool,
    /// Field centre: RA in degrees or hours (01:33:50.9); otherwise the
    /// object name gives it.
    #[arg(long, requires = "field_dec", allow_hyphen_values = true, value_parser = parse_field_ra)]
    field_ra: Option<f64>,
    /// Dec in degrees, decimal or +30:39:36.
    #[arg(long, requires = "field_ra", allow_hyphen_values = true, value_parser = parse_field_dec)]
    field_dec: Option<f64>,
    /// Target name (M 33, NGC 7000, LBN 437...); default: the OBJECT card.
    #[arg(long)]
    object: Option<String>,
    /// Look a target up online (NED, SIMBAD) when the built-in table lacks it.
    #[arg(long)]
    online_lookup: bool,
    /// Names of the light folders, for suggesting a target (set by the command).
    #[arg(skip)]
    folders: Vec<String>,
}

fn parse_field_ra(text: &str) -> std::result::Result<f64, String> {
    fulgur_catalog::sky::parse_ra(text)
        .ok_or_else(|| "use degrees (23.462) or hours (01:33:50.9)".into())
}

fn parse_field_dec(text: &str) -> std::result::Result<f64, String> {
    fulgur_catalog::sky::parse_dec(text).ok_or_else(|| "use degrees (+30.66 or +30:39:36)".into())
}

impl SkyArgs {
    /// The names of the light folders, from which a target is suggested when
    /// the headers name none (ADR-167).
    pub(crate) fn with_folders(mut self, inputs: &[PathBuf]) -> Self {
        self.folders = inputs
            .iter()
            .flat_map(|path| [Some(path.as_path()), path.parent()])
            .flatten()
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect();
        self
    }

    /// A Gaia catalogue was given, so the master can get a WCS (ADR-155).
    pub(crate) fn has_catalogue(&self) -> bool {
        self.gaia_db.is_some() || self.gaia_download
    }

    pub(crate) fn options(
        &self,
        inspection: &fulgur_pipeline::inspect::InspectReport,
    ) -> Result<fulgur_pipeline::register::SkyOptions> {
        let choice = fulgur_catalog::sky::SkyChoice {
            sources: fulgur_catalog::GaiaSources {
                pixinsight: self.gaia_db.clone(),
                cache: self
                    .gaia_cache
                    .clone()
                    .unwrap_or_else(|| std::env::temp_dir().join("fulgurstack_gaia_cache")),
                download: self.gaia_download,
            },
            centre: self.field_ra.zip(self.field_dec),
            object: self.object.clone(),
            online: self.online_lookup,
        };
        let say = std::sync::Arc::new(|line: String| println!("Sky registration: {line}."));
        let (options, position) = fulgur_catalog::sky::sky_options(&choice, inspection, say)
            .map_err(|message| {
                if choice.object.is_none()
                    && choice.centre.is_none()
                    && message.contains("No field centre")
                {
                    if let Some(guess) = fulgur_catalog::sky::target_guess(inspection, &self.folders)
                    {
                        println!(
                            "Sky registration: the built-in catalogue found {} ({}) in the {}. Confirm it with --object \"{}\", or give another --object or --field-ra/--field-dec.",
                            guess.matched, guess.catalogue, guess.from, guess.matched
                        );
                    }
                }
                // The cause itself, then what sky registration needs (B-21).
                Error::Message(format!(
                    "Sky registration: {}. It needs Gaia stars (--gaia-db or --gaia-download) and a field centre (--field-ra/--field-dec or --object).",
                    message.trim_end_matches('.')
                ))
            })?;
        println!(
            "Sky registration: field centre {} from {}.",
            fulgur_pipeline::sky::catalogue::position_text(position.ra, position.dec),
            fulgur_catalog::sky::source_label(&position)
        );
        Ok(options)
    }
}
