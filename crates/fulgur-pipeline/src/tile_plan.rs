//! Choose only previously verified tile geometries before allocating output images.
use crate::integrate::{IntegrationConfig, TileMode};
use fulgur_core::telemetry::SystemMemory;
use fulgur_core::{Error, Result};
use serde::Serialize;

const MIB: u64 = 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct TileCandidate {
    pub requested_size_px: usize,
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
}

impl TileWorkload {
    fn estimate(&self, width: usize, height: usize) -> usize {
        let output = self.width.saturating_mul(self.height).saturating_mul(36);
        let samples = self
            .sources
            .saturating_add(3)
            .saturating_mul(width)
            .saturating_mul(height)
            .saturating_mul(12);
        let side =
            (((width + height) as f64 * self.inverse_scale).ceil() as usize).saturating_add(40);
        let roi = self
            .workers
            .saturating_mul(side.saturating_pow(2))
            .saturating_mul(if self.gpu { 64 } else { 24 });
        (768 * MIB as usize)
            .saturating_add(output)
            .saturating_add(samples)
            .saturating_add(roi)
            .saturating_add(self.workers.saturating_mul(self.sources).saturating_mul(64))
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
        let candidates: Vec<_> = [512, 256, 128]
            .into_iter()
            .map(|size| {
                let width = size.min(self.width);
                let height = size.min(self.height);
                let estimated_buffer_bytes = self.estimate(width, height);
                TileCandidate {
                    requested_size_px: size,
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
                    "largest previously verified square fitting available RAM minus system reserve and configured ceiling",
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
        for (size, next) in [(512, 256), (256, 128)] {
            let needed = w.estimate(size, size) as u64;
            let exact = w
                .plan(&IntegrationConfig::default(), memory(needed + reserve))
                .unwrap();
            assert_eq!(exact.selected_width, size);
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
    fn canvas_scale_workers_and_gpu_affect_the_estimate() {
        let mut w = workload();
        let config = IntegrationConfig::default();
        let cpu = w.plan(&config, memory(24 * 1024 * MIB)).unwrap();
        w.gpu = true;
        let gpu = w.plan(&config, memory(24 * 1024 * MIB)).unwrap();
        assert!(gpu.estimated_buffer_bytes > cpu.estimated_buffer_bytes);
        w.inverse_scale = 1. / 0.33;
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
