//! Choose only previously verified tile geometries before allocating output images.
use crate::integrate::{IntegrationConfig, TileMode};
use fulgur_core::telemetry::SystemMemory;
use fulgur_core::{Error, Result};
use serde::Serialize;

const MIB: u64 = 1024 * 1024;

/// Frames wider than this use 1024 px wide tiles in Auto (ADR-118).
const WIDE_IMAGE_PX: usize = 4000;

#[derive(Debug, Serialize)]
pub struct TileCandidate {
    pub requested_width_px: usize,
    pub requested_height_px: usize,
    pub width: usize,
    pub height: usize,
    pub estimated_buffer_bytes: usize,
    pub fits_budget: bool,
}

#[derive(Debug, Serialize)]
pub struct MemoryPlan {
    pub mode: TileMode,
    pub system_memory: Option<SystemMemory>,
    pub system_reserve_bytes: Option<u64>,
    pub configured_budget_bytes: u64,
    pub effective_budget_bytes: u64,
    pub candidates: Vec<TileCandidate>,
    pub selected_width: usize,
    pub selected_height: usize,
    pub estimated_buffer_bytes: usize,
    pub decision: &'static str,
    pub integration_buffer_bytes: usize,
    pub fits_cache_capacity_bytes: usize,
    pub fits_cache_transient_bytes: usize,
}

impl MemoryPlan {
    /// Cache uses only remaining headroom; it never shrinks an already chosen tile.
    pub fn allocate_cache(
        &mut self,
        requested_mib: usize,
        sources: usize,
        workers: usize,
        max_width: usize,
    ) {
        self.fits_cache_capacity_bytes = 0;
        self.fits_cache_transient_bytes = 0;
        self.estimated_buffer_bytes = self.integration_buffer_bytes;
        if self.system_memory.is_none() || requested_mib == 0 {
            return;
        }
        let side = fulgur_core::fits_cache::BLOCK_SIDE;
        let transient = workers
            .saturating_mul(max_width.saturating_add(side))
            .saturating_mul(side)
            .saturating_mul(2);
        let headroom = self
            .effective_budget_bytes
            .saturating_sub(self.integration_buffer_bytes as u64)
            .saturating_sub(transient as u64) as usize;
        let capacity = fulgur_core::fits_cache::usable_capacity(
            (requested_mib * 1024 * 1024).min(headroom),
            sources,
        );
        if capacity > 0 {
            self.fits_cache_capacity_bytes = capacity;
            self.fits_cache_transient_bytes = transient;
            self.estimated_buffer_bytes = self.integration_buffer_bytes + capacity + transient;
        }
    }
}

/// Conservative existing buffer model, including all three final RGB images,
/// runtime/catalog reserve, frame-major tile samples, ROI workers and row scratch.
pub struct TileWorkload {
    pub width: usize,
    pub height: usize,
    pub sources: usize,
    pub workers: usize,
    pub inverse_scale: f64,
    pub gpu: bool,
    /// Device buffers of GPU pixel rejection; on an integrated GPU they are
    /// system memory (ADR-090).
    pub gpu_reduction_bytes: usize,
    pub distortion_margin: usize,
}

impl TileWorkload {
    /// Auto order, first fitting shape wins. Tile width sets the reading work
    /// on wide frames and height only adds memory, so CPU stacks use low tiles.
    /// GPU stacks keep squares: lower tiles mean more dispatches, not measured.
    fn auto_shapes(&self) -> &'static [(usize, usize)] {
        if self.gpu || self.gpu_reduction_bytes > 0 {
            &[(512, 512), (256, 256), (128, 128)]
        } else if self.width > WIDE_IMAGE_PX {
            &[(1024, 256), (512, 256), (256, 256), (128, 128)]
        } else {
            &[(512, 256), (256, 256), (128, 128)]
        }
    }

    fn estimate(&self, width: usize, height: usize) -> usize {
        let output = self.width.saturating_mul(self.height).saturating_mul(36);
        // GPU pixel rejection overlaps reconstruction: two tiles of samples.
        let sample_sets = if self.gpu_reduction_bytes > 0 { 2 } else { 1 };
        let samples = self
            .sources
            .saturating_add(3)
            .saturating_mul(width)
            .saturating_mul(height)
            .saturating_mul(12)
            .saturating_mul(sample_sets);
        let side = (((width + height) as f64 * self.inverse_scale).ceil() as usize)
            .saturating_add(40)
            .saturating_add(self.distortion_margin.saturating_mul(2));
        let roi = self
            .workers
            .saturating_mul(side.saturating_pow(2))
            .saturating_mul(if self.gpu { 64 } else { 24 });
        let read_ahead = self
            .workers
            .saturating_mul(fulgur_core::fits::ROI_READ_BUFFER_BYTES);
        (768 * MIB as usize)
            .saturating_add(self.gpu_reduction_bytes)
            .saturating_add(output)
            .saturating_add(samples)
            .saturating_add(roi)
            .saturating_add(read_ahead)
            .saturating_add(self.workers.saturating_mul(self.sources).saturating_mul(64))
            .saturating_add(crate::integrate::rejection_packing_bytes(
                self.sources,
                self.workers,
            ))
    }

    pub fn plan(
        &self,
        config: &IntegrationConfig,
        memory: Option<SystemMemory>,
    ) -> Result<MemoryPlan> {
        config.validate()?;
        if self.width == 0
            || self.height == 0
            || self.workers == 0
            || !self.inverse_scale.is_finite()
            || self.inverse_scale <= 0.
        {
            return Err(Error::Invalid("Invalid tile memory workload."));
        }
        let memory = memory.filter(|m| m.total_bytes > 0 && m.available_bytes <= m.total_bytes);
        if config.tile_mode == TileMode::Auto && memory.is_none() {
            return Err(Error::Invalid(
                "Available physical RAM could not be measured. Use --tile-size 128, 256 or 512 with an explicit --memory-budget-mib.",
            ));
        }
        let reserve = memory.map(|m| (2 * 1024 * MIB).max(m.total_bytes.div_ceil(10)));
        let configured_budget_bytes = config.memory_budget_mib as u64 * MIB;
        // Already resident pipeline data is deliberately not added back to available
        // RAM. Counting it again in the model leaves an additional safety margin.
        let effective_budget_bytes = memory
            .zip(reserve)
            .map_or(configured_budget_bytes, |(m, r)| {
                configured_budget_bytes.min(m.available_bytes.saturating_sub(r))
            });
        let candidates: Vec<_> = self
            .auto_shapes()
            .iter()
            .map(|&(requested_width_px, requested_height_px)| {
                let width = requested_width_px.min(self.width);
                let height = requested_height_px.min(self.height);
                let estimated_buffer_bytes = self.estimate(width, height);
                TileCandidate {
                    requested_width_px,
                    requested_height_px,
                    width,
                    height,
                    estimated_buffer_bytes,
                    fits_budget: (estimated_buffer_bytes as u64) <= effective_budget_bytes,
                }
            })
            .collect();
        let (width, height, estimated, decision) = match config.tile_mode {
            TileMode::Auto => {
                let c = candidates.iter().find(|c| c.fits_budget).ok_or(Error::Invalid(
                    "Insufficient available RAM after the system reserve: even a 128 x 128 tile cannot fit. Close other applications or reduce --threads.",
                ))?;
                (
                    c.width,
                    c.height,
                    c.estimated_buffer_bytes,
                    "first shape of the automatic order fitting available RAM minus system reserve and configured ceiling",
                )
            }
            TileMode::Manual => {
                let width = config.tile_width.min(self.width);
                let height = config.tile_height.min(self.height);
                let estimate = self.estimate(width, height);
                if estimate as u64 > effective_budget_bytes {
                    return Err(Error::Invalid(
                        "Requested manual tile exceeds the RAM budget after the system reserve. Choose a smaller tile or close other applications; no automatic resizing was applied.",
                    ));
                }
                (
                    width,
                    height,
                    estimate,
                    if memory.is_some() {
                        "manual geometry fits available RAM minus system reserve and configured ceiling"
                    } else {
                        "manual geometry; system RAM measurement unavailable, configured ceiling only"
                    },
                )
            }
        };
        Ok(MemoryPlan {
            mode: config.tile_mode,
            system_memory: memory,
            system_reserve_bytes: reserve,
            configured_budget_bytes,
            effective_budget_bytes,
            candidates,
            selected_width: width,
            selected_height: height,
            estimated_buffer_bytes: estimated,
            decision,
            integration_buffer_bytes: estimated,
            fits_cache_capacity_bytes: 0,
            fits_cache_transient_bytes: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workload() -> TileWorkload {
        TileWorkload {
            width: 4000,
            height: 4000,
            sources: 940,
            workers: 16,
            inverse_scale: 1.,
            gpu: false,
            gpu_reduction_bytes: 0,
            distortion_margin: 0,
        }
    }
    fn memory(available: u64) -> Option<SystemMemory> {
        Some(SystemMemory {
            total_bytes: 32 * 1024 * MIB,
            available_bytes: available,
        })
    }
    #[test]
    fn automatic_choice_at_exact_budget_boundaries() {
        let w = workload();
        let reserve = (32 * 1024 * MIB).div_ceil(10);
        for ((width, height), next) in [((512, 256), 256), ((256, 256), 128)] {
            let needed = w.estimate(width, height) as u64;
            let exact = w
                .plan(&IntegrationConfig::default(), memory(needed + reserve))
                .unwrap();
            assert_eq!(
                (exact.selected_width, exact.selected_height),
                (width, height)
            );
            assert_eq!(
                exact.estimated_buffer_bytes as u64,
                exact.effective_budget_bytes
            );
            let below = w
                .plan(&IntegrationConfig::default(), memory(needed + reserve - 1))
                .unwrap();
            assert_eq!(below.selected_width, next);
        }
        let needed = w.estimate(128, 128) as u64;
        assert_eq!(
            w.plan(&IntegrationConfig::default(), memory(needed + reserve))
                .unwrap()
                .selected_width,
            128
        );
        assert!(
            w.plan(&IntegrationConfig::default(), memory(needed + reserve - 1))
                .is_err()
        );
        assert!(w.plan(&IntegrationConfig::default(), memory(1)).is_err());
    }
    #[test]
    fn system_reserve_ceiling_and_manual_override_are_enforced() {
        let w = workload();
        let mut config = IntegrationConfig {
            memory_budget_mib: 2048,
            ..Default::default()
        };
        let p = w.plan(&config, memory(24 * 1024 * MIB)).unwrap();
        assert_eq!(p.effective_budget_bytes, 2048 * MIB);
        assert_eq!(p.selected_width, 128);
        config.tile_mode = TileMode::Manual;
        assert!(w.plan(&config, memory(24 * 1024 * MIB)).is_err());
        config.tile_width = 128;
        config.tile_height = 128;
        assert_eq!(
            w.plan(&config, memory(24 * 1024 * MIB))
                .unwrap()
                .selected_width,
            128
        );
        assert!(w.plan(&config, memory(2 * 1024 * MIB)).is_err());
        let small = Some(SystemMemory {
            total_bytes: 8 * 1024 * MIB,
            available_bytes: 6 * 1024 * MIB,
        });
        assert_eq!(
            w.plan(&config, small).unwrap().system_reserve_bytes,
            Some(2 * 1024 * MIB)
        );
        assert!(w.plan(&config, None).is_ok());
        config.tile_mode = TileMode::Auto;
        assert!(w.plan(&config, None).is_err());
        assert!(
            w.plan(
                &config,
                Some(SystemMemory {
                    total_bytes: 0,
                    available_bytes: 0
                })
            )
            .is_err()
        );
    }
    #[test]
    fn automatic_shape_follows_frame_width_and_engine() {
        let mut w = workload();
        let auto = IntegrationConfig::default();
        let plenty = memory(24 * 1024 * MIB);
        let shape = |w: &TileWorkload| {
            let p = w.plan(&auto, plenty).unwrap();
            (p.selected_width, p.selected_height)
        };
        assert_eq!(shape(&w), (512, 256));
        w.width = 4001;
        assert_eq!(shape(&w), (1024, 256));
        // Short of memory on a wide frame, the narrower tile comes next.
        let reserve = (32 * 1024 * MIB).div_ceil(10);
        let needed = w.estimate(1024, 256) as u64;
        let p = w.plan(&auto, memory(needed + reserve - 1)).unwrap();
        assert_eq!((p.selected_width, p.selected_height), (512, 256));
        assert_eq!(p.candidates.len(), 4);
        assert_eq!(p.candidates[0].requested_width_px, 1024);
        assert_eq!(p.candidates[0].requested_height_px, 256);
        // GPU stacks keep the squares they were measured with.
        w.gpu = true;
        assert_eq!(shape(&w), (512, 512));
        w.gpu = false;
        w.gpu_reduction_bytes = 1;
        assert_eq!(shape(&w), (512, 512));
    }
    #[test]
    fn canvas_scale_workers_and_gpu_affect_the_estimate() {
        let mut w = workload();
        let config = IntegrationConfig::default();
        let cpu = w.plan(&config, memory(24 * 1024 * MIB)).unwrap();
        w.workers = 24;
        let more = w.plan(&config, memory(24 * 1024 * MIB)).unwrap();
        assert!(more.estimated_buffer_bytes > cpu.estimated_buffer_bytes);
        assert!(more.estimated_buffer_bytes <= more.effective_budget_bytes as usize);
        w.workers = 16;
        w.gpu = true;
        let gpu = w.plan(&config, memory(24 * 1024 * MIB)).unwrap();
        assert!(gpu.estimated_buffer_bytes > cpu.estimated_buffer_bytes);
        w.inverse_scale = 1. / 0.30;
        let scaled = w.plan(&config, memory(24 * 1024 * MIB)).unwrap();
        assert!(
            scaled.candidates[0].estimated_buffer_bytes > gpu.candidates[0].estimated_buffer_bytes
        );
        assert!(scaled.selected_width < gpu.selected_width);
        w.width = 91;
        w.height = 73;
        let small = w.plan(&config, memory(24 * 1024 * MIB)).unwrap();
        assert_eq!((small.selected_width, small.selected_height), (91, 73));
        w.width = usize::MAX;
        w.height = 2;
        assert!(w.plan(&config, memory(24 * 1024 * MIB)).is_err());
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    #[test]
    fn cache_uses_only_remaining_measured_headroom_without_changing_tile_geometry() {
        let work = TileWorkload {
            width: 4000,
            height: 4000,
            sources: 940,
            workers: 16,
            inverse_scale: 1.,
            gpu: false,
            gpu_reduction_bytes: 0,
            distortion_margin: 0,
        };
        let config = IntegrationConfig::default();
        let memory = SystemMemory {
            total_bytes: 32 * 1024 * MIB,
            available_bytes: 24 * 1024 * MIB,
        };
        let mut p = work.plan(&config, Some(memory)).unwrap();
        let base = p.estimated_buffer_bytes;
        p.allocate_cache(1024, 940, 16, 2160);
        assert_eq!(p.fits_cache_capacity_bytes, 1024 * 1024 * 1024);
        assert_eq!(p.integration_buffer_bytes, base);
        assert_eq!(p.selected_width, 512);
        assert_eq!(
            p.estimated_buffer_bytes,
            base + p.fits_cache_capacity_bytes + p.fits_cache_transient_bytes
        );
        assert!(p.estimated_buffer_bytes as u64 <= p.effective_budget_bytes);
        let mut tight = work.plan(&config, Some(memory)).unwrap();
        tight.effective_budget_bytes = base as u64 + 256 * MIB;
        tight.allocate_cache(1024, 940, 16, 2160);
        assert!(
            tight.fits_cache_capacity_bytes > 0
                && tight.fits_cache_capacity_bytes < 256 * MIB as usize
        );
        assert!(tight.estimated_buffer_bytes as u64 <= tight.effective_budget_bytes);
        let mut tiny = work.plan(&config, Some(memory)).unwrap();
        tiny.effective_budget_bytes = base as u64;
        tiny.allocate_cache(1024, 940, 16, 2160);
        assert_eq!(tiny.fits_cache_capacity_bytes, 0);
        let mut unknown = work
            .plan(
                &IntegrationConfig {
                    tile_mode: TileMode::Manual,
                    ..config
                },
                None,
            )
            .unwrap();
        unknown.allocate_cache(1024, 940, 16, 2160);
        assert_eq!(unknown.fits_cache_capacity_bytes, 0);
    }
}
