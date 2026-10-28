//! Bounded, read-only original-byte blocks. No decoded/calibrated/RGB image cache.
#[cfg(test)]
use crate::fits::read_roi;
use crate::fits::{FitsHeader, read_roi_with_stats_into, validate_roi};
use crate::{Error, RawFrame, Result, check_cancelled};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

pub const BLOCK_SIDE: usize = 128;
const ENTRY_OVERHEAD: usize = 256;
const SOURCE_OVERHEAD: usize = 1024;
const BLOCK_ACCOUNT: usize = BLOCK_SIDE * BLOCK_SIDE * 2 + ENTRY_OVERHEAD;

/// Include conservative map/entry overhead in the capacity, not only raw payload.
pub fn usable_capacity(requested: usize, sources: usize) -> usize {
    let requested = requested.min(1024 * 1024 * 1024);
    if sources == 0 || requested / sources < SOURCE_OVERHEAD + BLOCK_ACCOUNT {
        0
    } else {
        requested
    }
}

#[derive(Default, Debug, Serialize)]
pub struct CacheStats {
    pub enabled: bool,
    pub capacity_accounted_bytes: usize,
    pub retained_accounted_bytes: usize,
    /// Conservative sum of each source's peak; peaks need not occur simultaneously.
    pub sum_source_peak_accounted_bytes: usize,
    pub retained_payload_bytes: usize,
    pub retained_blocks: usize,
    pub roi_requests: u64,
    pub block_hits: u64,
    pub block_misses: u64,
    pub evictions: u64,
    pub requested_roi_bytes: u64,
    pub hit_requested_bytes: u64,
    /// Bytes requested from read-only files, not measured physical SSD traffic.
    pub source_bytes_read: u64,
    pub read_calls: u64,
    pub file_opens: u64,
    pub metadata_checks: u64,
}
struct Entry {
    bytes: Vec<u8>,
    used: u64,
}
#[derive(Default)]
struct SourceCache {
    entries: BTreeMap<(usize, usize), Entry>,
    used_bytes: usize,
    peak_bytes: usize,
    clock: u64,
    stamp: Option<Option<SystemTime>>,
    stats: CacheStats,
}
pub struct FitsBlockCache {
    sources: HashMap<PathBuf, Mutex<SourceCache>>,
    capacity: usize,
    quota: usize,
}
impl FitsBlockCache {
    pub fn new(paths: &[PathBuf], requested: usize) -> Self {
        let sources: HashMap<_, _> = paths
            .iter()
            .map(|p| (p.clone(), Mutex::new(SourceCache::default())))
            .collect();
        let capacity = usable_capacity(requested, sources.len());
        let quota = if capacity == 0 {
            0
        } else {
            capacity / sources.len() - SOURCE_OVERHEAD
        };
        Self {
            sources,
            capacity,
            quota,
        }
    }

    pub fn read_roi(
        &self,
        path: &Path,
        header: &FitsHeader,
        roi: [usize; 4],
        cancel: &AtomicBool,
    ) -> Result<RawFrame> {
        self.read_roi_into(path, header, roi, cancel, Vec::new())
    }

    /// As `read_roi`, decoding into reused storage (e.g. the previous ROI's
    /// pixels). Every returned sample is overwritten; contents never leak.
    pub fn read_roi_into(
        &self,
        path: &Path,
        header: &FitsHeader,
        roi: [usize; 4],
        cancel: &AtomicBool,
        mut pixels: Vec<f32>,
    ) -> Result<RawFrame> {
        check_cancelled(cancel)?;
        validate_roi(header, roi)?;
        let state = self
            .sources
            .get(path)
            .ok_or(Error::Invalid("FITS cache source was not registered."))?;
        let mut state = state
            .lock()
            .map_err(|_| Error::Invalid("FITS cache lock failed."))?;
        let [x, y, width, height] = roi;
        state.stats.roi_requests += 1;
        state.stats.requested_roi_bytes += (width * height * 2) as u64;
        if self.capacity == 0 {
            let (frame, calls, bytes) =
                read_roi_with_stats_into(path, header, roi, cancel, pixels)?;
            state.stats.file_opens += 1;
            state.stats.metadata_checks += 1;
            state.stats.read_calls += calls;
            state.stats.source_bytes_read += bytes;
            return Ok(frame);
        }
        // Even all-hit reads reject a modified source rather than serving stale data.
        let metadata = std::fs::metadata(path)?;
        state.stats.metadata_checks += 1;
        let stamp = metadata.modified().ok();
        if metadata.len() != header.file_bytes
            || state.stamp.is_some_and(|previous| previous != stamp)
        {
            return Err(Error::Invalid(
                "Source FITS changed during cached processing.",
            ));
        }
        state.stamp = Some(stamp);
        pixels.clear();
        pixels.resize(width * height, 0.);
        let mut file: Option<File> = None;
        let end_bx = (x + width).div_ceil(BLOCK_SIDE);
        for by in y / BLOCK_SIDE..(y + height).div_ceil(BLOCK_SIDE) {
            check_cancelled(cancel)?;
            let mut bx = x / BLOCK_SIDE;
            while bx < end_bx {
                state.clock += 1;
                let clock = state.clock;
                if let Some(block) = state.entries.get_mut(&(bx, by)) {
                    block.used = clock;
                    let bytes_used = copy_block(&block.bytes, [bx, by], header, roi, &mut pixels);
                    state.stats.hit_requested_bytes += bytes_used as u64;
                    state.stats.block_hits += 1;
                    bx += 1;
                    continue;
                }
                let first = bx;
                bx += 1;
                while bx < end_bx && !state.entries.contains_key(&(bx, by)) {
                    bx += 1;
                }
                let left = first * BLOCK_SIDE;
                let top = by * BLOCK_SIDE;
                let run_width = (bx * BLOCK_SIDE).min(header.info.width) - left;
                let run_height = BLOCK_SIDE.min(header.info.height - top);
                let mut bytes = vec![0; run_width * run_height * 2];
                if file.is_none() {
                    let opened = File::open(path)?;
                    state.stats.file_opens += 1;
                    let meta = opened.metadata()?;
                    state.stats.metadata_checks += 1;
                    if meta.len() != header.file_bytes || meta.modified().ok() != stamp {
                        return Err(Error::Invalid(
                            "Source FITS changed while opening cached data.",
                        ));
                    }
                    file = Some(opened);
                }
                let opened = file.as_mut().unwrap();
                // Merge adjacent block misses into one read per source row. Do not
                // read the large gaps between rows or materialize a whole image.
                for row in 0..run_height {
                    check_cancelled(cancel)?;
                    let offset =
                        header.data_offset + ((top + row) * header.info.width + left) as u64 * 2;
                    opened.seek(SeekFrom::Start(offset))?;
                    opened
                        .read_exact(&mut bytes[row * run_width * 2..(row + 1) * run_width * 2])?;
                    state.stats.read_calls += 1;
                }
                state.stats.source_bytes_read += bytes.len() as u64;
                for block_x in first..bx {
                    let block_width = BLOCK_SIDE.min(header.info.width - block_x * BLOCK_SIDE);
                    let mut block = vec![0; block_width * run_height * 2];
                    let offset = (block_x - first) * BLOCK_SIDE * 2;
                    for row in 0..run_height {
                        block[row * block_width * 2..(row + 1) * block_width * 2].copy_from_slice(
                            &bytes[row * run_width * 2 + offset
                                ..row * run_width * 2 + offset + block_width * 2],
                        );
                    }
                    copy_block(&block, [block_x, by], header, roi, &mut pixels);
                    state.stats.block_misses += 1;
                    let cost = block.len() + ENTRY_OVERHEAD;
                    while state.used_bytes + cost > self.quota {
                        let key = *state.entries.iter().min_by_key(|(_, e)| e.used).unwrap().0;
                        let entry = state.entries.remove(&key).unwrap();
                        state.used_bytes -= entry.bytes.len() + ENTRY_OVERHEAD;
                        state.stats.evictions += 1;
                    }
                    state.clock += 1;
                    let used = state.clock;
                    state
                        .entries
                        .insert((block_x, by), Entry { bytes: block, used });
                    state.used_bytes += cost;
                    state.peak_bytes = state.peak_bytes.max(state.used_bytes);
                }
            }
        }
        let mut result_header = header.clone();
        result_header.info.width = width;
        result_header.info.height = height;
        Ok(RawFrame {
            header: result_header,
            pixels,
        })
    }

    pub fn stats(&self) -> Result<CacheStats> {
        let overhead = if self.capacity == 0 {
            0
        } else {
            self.sources.len() * SOURCE_OVERHEAD
        };
        let mut total = CacheStats {
            enabled: self.capacity > 0,
            capacity_accounted_bytes: self.capacity,
            retained_accounted_bytes: overhead,
            sum_source_peak_accounted_bytes: overhead,
            ..Default::default()
        };
        for source in self.sources.values() {
            let source = source
                .lock()
                .map_err(|_| Error::Invalid("FITS cache lock failed."))?;
            total.retained_accounted_bytes += source.used_bytes;
            total.sum_source_peak_accounted_bytes += source.peak_bytes;
            total.retained_payload_bytes += source
                .entries
                .values()
                .map(|e| e.bytes.len())
                .sum::<usize>();
            total.retained_blocks += source.entries.len();
            let s = &source.stats;
            total.roi_requests += s.roi_requests;
            total.block_hits += s.block_hits;
            total.block_misses += s.block_misses;
            total.evictions += s.evictions;
            total.requested_roi_bytes += s.requested_roi_bytes;
            total.hit_requested_bytes += s.hit_requested_bytes;
            total.source_bytes_read += s.source_bytes_read;
            total.read_calls += s.read_calls;
            total.file_opens += s.file_opens;
            total.metadata_checks += s.metadata_checks;
        }
        Ok(total)
    }
}

fn copy_block(
    bytes: &[u8],
    block: [usize; 2],
    header: &FitsHeader,
    roi: [usize; 4],
    output: &mut [f32],
) -> usize {
    let [x, y, width, height] = roi;
    let left = block[0] * BLOCK_SIDE;
    let top = block[1] * BLOCK_SIDE;
    let block_width = BLOCK_SIDE.min(header.info.width - left);
    let right = (left + block_width).min(x + width);
    let bottom = (top + BLOCK_SIDE).min(y + height).min(header.info.height);
    let mut count = 0;
    for row in top.max(y)..bottom {
        let start = left.max(x);
        let source = ((row - top) * block_width + start - left) * 2;
        for (out, pair) in output[(row - y) * width + start - x..(row - y) * width + right - x]
            .iter_mut()
            .zip(bytes[source..source + (right - start) * 2].chunks_exact(2))
        {
            *out = f32::from(i16::from_be_bytes([pair[0], pair[1]])) + header.bzero;
        }
        count += (right - start) * 2;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fits::read_header;
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::sync::atomic::Ordering;

    #[test]
    fn bounded_read_ahead_matches_frozen_row_reader_at_gaps_buffers_and_eof() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.fit");
        let header = source(&path);
        let original = Sha256::digest(fs::read(&path).unwrap());
        for [x, y, width, height] in [
            [0, 0, 259, 263],
            [2, 4, 251, 259],
            [252, 256, 7, 7],
            [0, 130, 6, 127],
            [0, 0, 257, 259],
        ] {
            // Frozen original seek/read per row, independent of production buffering.
            let mut file = File::open(&path).unwrap();
            let mut row = vec![0_u8; width * 2];
            let mut expected = vec![];
            for yy in y..y + height {
                file.seek(SeekFrom::Start(
                    header.data_offset + ((yy * header.info.width + x) * 2) as u64,
                ))
                .unwrap();
                file.read_exact(&mut row).unwrap();
                expected.extend(
                    row.chunks_exact(2)
                        .map(|p| f32::from(i16::from_be_bytes([p[0], p[1]])) + header.bzero),
                );
            }
            let (actual, calls, bytes) = crate::fits::read_roi_with_stats(
                &path,
                &header,
                [x, y, width, height],
                &AtomicBool::new(false),
            )
            .unwrap();
            assert_eq!(actual.pixels, expected);
            assert_eq!(
                (actual.header.info.width, actual.header.info.height),
                (width, height)
            );
            assert!(calls > 0 && calls < height as u64);
            assert!(bytes >= (width * height * 2) as u64);
        }
        assert_eq!(original, Sha256::digest(fs::read(path).unwrap()));
    }

    fn source(path: &Path) -> FitsHeader {
        let mut bytes = Vec::new();
        for (key, value) in [
            ("SIMPLE", "T"),
            ("BITPIX", "16"),
            ("NAXIS", "2"),
            ("NAXIS1", "259"),
            ("NAXIS2", "263"),
            ("BZERO", "32768"),
            ("BAYERPAT", "'GRBG'"),
            ("EXPTIME", "10"),
            ("BIAS", "2064"),
        ] {
            bytes.extend(format!("{:<80}", format!("{key:<8}= {value:>20}")).as_bytes());
        }
        bytes.extend(format!("{:<80}", "END").as_bytes());
        bytes.resize(2880, b' ');
        for i in 0..259 * 263 {
            let v = [0, 2064, 32767, 32768, 65535, (i * 37) % 65536][i % 6];
            bytes.extend(((v as i32 - 32768) as i16).to_be_bytes());
        }
        bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
        fs::write(path, bytes).unwrap();
        read_header(path).unwrap()
    }
    #[test]
    fn cached_blocks_match_direct_signed_decode_at_boundaries_and_preserve_sources() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.fit");
        let header = source(&path);
        let before = Sha256::digest(fs::read(&path).unwrap());
        let cache = FitsBlockCache::new(std::slice::from_ref(&path), 2 * 1024 * 1024);
        let cancel = AtomicBool::new(false);
        for roi in [
            [0, 0, 259, 263],
            [2, 4, 257, 259],
            [124, 126, 134, 136],
            [252, 254, 7, 9],
            [0, 0, 6, 6],
        ] {
            let expected = read_roi(&path, &header, roi, &cancel).unwrap();
            let actual = cache.read_roi(&path, &header, roi, &cancel).unwrap();
            assert_eq!(expected.pixels, actual.pixels);
            assert_eq!(
                (actual.header.info.width, actual.header.info.height),
                (roi[2], roi[3])
            );
        }
        let s = cache.stats().unwrap();
        assert!(s.block_hits > 0 && s.hit_requested_bytes > 0);
        assert!(s.sum_source_peak_accounted_bytes <= s.capacity_accounted_bytes);
        let calls = s.read_calls;
        let opens = s.file_opens;
        cache
            .read_roi(&path, &header, [2, 4, 257, 259], &cancel)
            .unwrap();
        assert_eq!(cache.stats().unwrap().read_calls, calls);
        assert_eq!(cache.stats().unwrap().file_opens, opens);
        assert_eq!(before, Sha256::digest(fs::read(&path).unwrap()));
    }
    #[test]
    fn eviction_disabled_mode_and_parallel_source_quotas_remain_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.fit");
        let header = source(&path);
        let cancel = AtomicBool::new(false);
        let cache =
            FitsBlockCache::new(std::slice::from_ref(&path), SOURCE_OVERHEAD + BLOCK_ACCOUNT);
        for x in [0, 128, 0] {
            let roi = [x, 0, 20, 20];
            assert_eq!(
                cache.read_roi(&path, &header, roi, &cancel).unwrap().pixels,
                read_roi(&path, &header, roi, &cancel).unwrap().pixels
            );
        }
        let s = cache.stats().unwrap();
        assert_eq!(s.block_misses, 3);
        assert_eq!(s.evictions, 2);
        assert!(s.sum_source_peak_accounted_bytes <= s.capacity_accounted_bytes);
        let disabled = FitsBlockCache::new(std::slice::from_ref(&path), 0);
        disabled
            .read_roi(&path, &header, [0, 0, 20, 20], &cancel)
            .unwrap();
        let s = disabled.stats().unwrap();
        assert!(!s.enabled);
        assert!(s.read_calls < 20 && s.read_calls > 0);
        assert!(s.source_bytes_read >= 800 && s.source_bytes_read <= header.file_bytes);
        assert_eq!(s.file_opens, 1);
        let second = dir.path().join("b.fit");
        source(&second);
        let shared = FitsBlockCache::new(&[path.clone(), second.clone()], 1024 * 1024);
        use rayon::prelude::*;
        (0..32).into_par_iter().for_each(|i| {
            let p = if i % 2 == 0 { &path } else { &second };
            let roi = [((i % 4) * 64) as usize, 126, 65, 65];
            assert_eq!(
                shared.read_roi(p, &header, roi, &cancel).unwrap().pixels,
                read_roi(p, &header, roi, &cancel).unwrap().pixels
            );
        });
        let s = shared.stats().unwrap();
        assert_eq!(s.roi_requests, 32);
        assert!(s.sum_source_peak_accounted_bytes <= s.capacity_accounted_bytes);
        assert!(s.block_hits > 0);
        assert_eq!(usable_capacity(1, 2), 0);
        assert_eq!(usable_capacity(usize::MAX, 1), 1024 * 1024 * 1024);
    }
    #[test]
    fn cancellation_invalid_rois_and_source_mutations_never_return_stale_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.fit");
        let header = source(&path);
        let cache = FitsBlockCache::new(std::slice::from_ref(&path), 1024 * 1024);
        let cancel = AtomicBool::new(true);
        assert!(
            cache
                .read_roi(&path, &header, [0, 0, 20, 20], &cancel)
                .is_err()
        );
        assert_eq!(cache.stats().unwrap().roi_requests, 0);
        cancel.store(false, Ordering::Relaxed);
        assert!(
            cache
                .read_roi(&path, &header, [1, 0, 20, 20], &cancel)
                .is_err()
        );
        cache
            .read_roi(&path, &header, [0, 0, 20, 20], &cancel)
            .unwrap();
        let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_times(fs::FileTimes::new().set_modified(SystemTime::UNIX_EPOCH))
            .unwrap();
        assert!(
            cache
                .read_roi(&path, &header, [0, 0, 20, 20], &cancel)
                .is_err()
        );
        file.set_len(header.file_bytes - 1).unwrap();
        assert!(
            cache
                .read_roi(&path, &header, [0, 0, 20, 20], &cancel)
                .is_err()
        );
    }
}
