use std::cell::RefCell;
use std::fmt::Write;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::io::Write as _;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use itoa::Buffer as ItoaBuf; // int → str without alloc
use memchr::memchr;
use memchr::memchr_iter;
use memmap2::Mmap;
// SIMD newline search
use rayon::prelude::*; // data-parallel helpers
use serde_json::Value;
use serde_json::to_string_pretty;
use simd_json::serde::from_slice as simd_parse; // SIMD parser // generic JSON value

const BUF_SIZE: usize = 1024 * 1024; // 1 MiB buffer

const BATCH_SIZE: usize = 64; // Process in batches

// ── 2. Prepare a 256 KiB buffered writer ─────────────────────
const BUF: usize = 256 * 1024;

// -------------------------------------------------------------------------
// Configuration for optimal parallel processing
// -------------------------------------------------------------------------
const CHUNK_SIZE: usize = 8192; // Lines per chunk for parallel processing
const MIN_PARALLEL_LINES: usize = 1000; // Minimum lines to justify parallelization
const WRITE_BUFFER_SIZE: usize = 2 * 1024 * 1024; // 2MB write buffer
const PARSE_BUFFER_SIZE: usize = 16384; // Per-thread parse buffer size
const OPTIMAL_CHUNK_SIZE: usize = 8192; // Tuned for CPU cache

const BOUNDARY_CHUNK_SIZE: usize = 256 * 1024; // 256KB chunks

// ------------------------ EXTREME UNRELATED OPTIMIZATION SECTION -------------------------- //
// ------------------------------------------------------------------
// Convert *.jsonl → pretty-printed JSON array (SIMD parsing)
// ------------------------------------------------------------------
pub fn convert_jsonl_to_pretty_array_optimized<P, Q>(input_path: P, output_path: Q) -> Result<()>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    // Memory-map input file
    let file: File = File::open(input_path)?;
    let mmap: Mmap = unsafe { Mmap::map(&file)? };

    let mut w: BufWriter<File> = BufWriter::with_capacity(512 * 1024, File::create(output_path)?);

    // Pre-allocate strings for common operations
    let mut fname_buf: String = String::with_capacity(256);

    // Write opening bracket
    w.write_all(b"[\n")?;

    // Process lines using manual newline search (faster than memchr for this pattern)
    let data: &[u8] = &mmap[..];
    let mut start: usize = 0;
    let mut first: bool = true;

    // Pre-allocate a reusable buffer for SIMD parsing
    let mut parse_buf: Vec<u8> = Vec::with_capacity(8192);

    while start < data.len() {
        // Find next newline using optimized loop
        let mut end = start;
        while end < data.len() && data[end] != b'\n' {
            end += 1;
        }

        let line_len: usize = end - start;

        // Skip empty lines
        if line_len > 0 {
            // Copy line to parse buffer (SIMD parser needs mutable slice)
            parse_buf.clear();
            parse_buf.extend_from_slice(&data[start..end]);

            // SIMD parse with direct buffer + Optimized filename:line_number concatenation
            if let Ok(mut json) = simd_parse::<Value>(&mut parse_buf)
                && let Some(obj) = json.as_object_mut()
                && let (Some(Value::String(fname)), Some(Value::Number(ln))) =
                    (obj.get("filename"), obj.get("line_number"))
                && let Some(line_num) = ln.as_u64()
            {
                // Reuse string buffer
                fname_buf.clear();
                fname_buf.push_str(fname);
                fname_buf.push(':');
                let _ = write!(&mut fname_buf, "{line_num}");
                obj.insert("filename".to_string(), Value::String(fname_buf.clone()));

                // Write separator
                if !first {
                    w.write_all(b",\n")?;
                }
                first = false;

                // Pretty-print to writer
                to_writer_pretty(&mut w, &json)?;
            }
        }

        // Move to next line
        start = end + 1;
    }

    // Close array and flush
    w.write_all(b"\n]")?;
    w.flush()?;

    Ok(())
}

// ──────────────────────────────────────────────────────────────────────
// High-throughput JSONL → pretty JSON array converter
//   • mmap + SIMD newline scan (memchr)
//   • simd-json parsing
//   • reusable scratch + filename buffer
//   • 256 KiB buffered writes
// ──────────────────────────────────────────────────────────────────────

#[rustfmt::skip]
pub fn convert_jsonl_to_pretty_array_optimized_hybrid<P, Q>(
    input_path: P,                     // source *.jsonl
    output_path: Q,                    // destination *.json
) -> Result<()>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    // ── 1. Map the input file read-only ───────────────────────────
    let file: File = File::open(input_path)?;
    let mmap: Mmap = unsafe { Mmap::map(&file)? };

    let out: File = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(output_path)?;
    let mut w: BufWriter<File> = BufWriter::with_capacity(BUF, out);

    // ── 3. Emit opening bracket of JSON array ────────────────────
    w.write_all(b"[\n")?;

    // ── 4. Reusable buffers to avoid per-line allocations ────────
    let mut scratch: Vec<u8> = Vec::with_capacity(4096);
    let mut fname_buf: String  = String::with_capacity(256);

    // ── 5. Scan mmap slice for newlines with memchr ──────────────
    let mut first: bool = true;              // track first element
    let mut pos: usize   = 0usize;            // current cursor

    while pos < mmap.len() {
        // Locate next '\n' or EOF
        let end: usize = memchr(b'\n', &mmap[pos..])
            .map_or(mmap.len(), |rel: usize| pos + rel);

        // Skip empty lines fast
        if end > pos {
            // Copy bytes into mutable scratch for SIMD parser
            scratch.clear();
            scratch.extend_from_slice(&mmap[pos..end]);

            // SIMD parse; skip malformed JSON lines
            // ----- Filename:line patch ----------------------
            if let Ok(mut json) = simd_parse::<Value>(&mut scratch) {
                if let (Some(fname), Some(ln)) =
                    (json.get("filename").and_then(Value::as_str),
                     json.get("line_number").and_then(Value::as_u64))
                {
                    fname_buf.clear();
                    fname_buf.push_str(fname);
                    
                    let _ = write!(fname_buf, ":{ln}");
                    
                    json["filename"] =
                        Value::String(fname_buf.clone());
                }

                // ----- Separator for array ---------------------
                if first { first = false } else { w.write_all(b",\n")? }

                // ----- Pretty-print record ---------------------
                to_writer_pretty(&mut w, &json)?;
            }
        }

        // Advance cursor past newline
        pos = end + 1;
    }

    // ── 6. Close JSON array and flush ────────────────────────────
    w.write_all(b"\n]")?;
    w.flush()?;
    Ok(())
}

#[inline]
fn fast_u64_to_string(mut n: u64, buf: &mut [u8; 20]) -> &str {
    let mut i = 20;
    if n == 0 {
        buf[19] = b'0';
        return unsafe { std::str::from_utf8_unchecked(&buf[19..]) };
    }

    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }

    unsafe { std::str::from_utf8_unchecked(&buf[i..]) }
}

pub fn convert_jsonl_to_pretty_array_ultra_optimized<P, Q>(
    input_path: P,
    output_path: Q,
) -> Result<()>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    // ── 1. Memory map input with read-ahead hints ────────────────
    let file: File = File::open(input_path)?;

    let mmap: Mmap = unsafe {
        let m: Mmap = Mmap::map(&file)?;

        // Hint for sequential read pattern
        m.advise(memmap2::Advice::Sequential)?;

        m
    };

    // ── 2. Optimal buffer size (1MB for modern SSDs) ────────────
    let buf_size: usize = 1024 * 1024;
    let out: File = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(output_path)?;
    let mut w = BufWriter::with_capacity(buf_size, out);

    // ── 3. Pre-allocated buffers with optimal sizes ─────────────
    let mut scratch: Vec<u8> = Vec::with_capacity(16384); // Larger scratch
    let mut fname_buf: String = String::with_capacity(512); // Larger filename buffer
    let mut u64_buf: [u8; 20] = [0u8; 20]; // Stack-allocated int conversion

    w.write_all(b"[\n")?;

    // ── 4. SIMD newline scan with batched processing ────────────
    let mut first: bool = true;
    let mut pos: usize = 0;
    let mut batch_count: i32 = 0;

    while pos < mmap.len() {
        // Process multiple lines before I/O
        let mut batch_output: Vec<u8> = Vec::with_capacity(64 * 1024);

        for _ in 0..BATCH_SIZE {
            if pos >= mmap.len() {
                break;
            }

            // memchr SIMD newline scan
            let end: usize = memchr(b'\n', &mmap[pos..]).map_or(mmap.len(), |rel: usize| pos + rel);

            if end > pos {
                // Zero-copy line extraction
                let line_slice: &[u8] = &mmap[pos..end];

                // Copy only once to mutable scratch
                scratch.clear();
                scratch.extend_from_slice(line_slice);

                // SIMD parse with early exit on failure
                // Optimized filename:line concatenation
                if let Ok(mut json) = simd_parse::<Value>(&mut scratch) {
                    if let (Some(fname), Some(ln)) = (
                        json.get("filename").and_then(Value::as_str),
                        json.get("line_number").and_then(Value::as_u64),
                    ) {
                        fname_buf.clear();
                        fname_buf.push_str(fname);
                        fname_buf.push(':');
                        // Fast integer conversion (no format! allocation)
                        fname_buf.push_str(fast_u64_to_string(ln, &mut u64_buf));

                        json["filename"] = Value::String(fname_buf.clone());
                    }

                    // Buffer the JSON instead of immediate write
                    if !first || batch_count > 0 {
                        batch_output.extend_from_slice(b",\n");
                    }

                    to_writer_pretty(&mut batch_output, &json)?;

                    if first {
                        first = false;
                    }

                    batch_count += 1;
                }
            }

            pos = end + 1;
        }

        // Batch write to minimize system calls
        if !batch_output.is_empty() {
            w.write_all(&batch_output)?;
        }
    }

    w.write_all(b"\n]")?;
    w.flush()?;
    Ok(())
}

// -------------------------------------------------------------------------
// Public entry point
// -------------------------------------------------------------------------
pub fn convert_jsonl_to_pretty_array_fastest<P, Q>(input_path: P, output_path: Q) -> Result<()>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    // ── 1. Memory-map with sequential access hint ────────────────────
    let file = File::open(input_path)?;
    let mmap = unsafe {
        let m = Mmap::map(&file)?;
        m.advise(memmap2::Advice::Sequential)?;
        m
    };
    let bytes = &mmap[..];

    // ── 2. Collect line boundaries with SIMD memchr ─────────────────
    let mut line_starts = vec![0usize];
    line_starts.extend(
        memchr_iter(b'\n', bytes)
            .map(|pos| pos + 1)
            .filter(|&pos| pos < bytes.len()),
    );

    // ── 3. Chunk lines for optimal parallelization ─────────────────
    let chunks: Vec<(usize, usize)> = line_starts
        .chunks(OPTIMAL_CHUNK_SIZE)
        .map(|chunk| {
            let start = chunk[0];
            let end = chunk
                .get(chunk.len().saturating_sub(1))
                .and_then(|&last_start| {
                    memchr(b'\n', &bytes[last_start..]).map(|rel| last_start + rel)
                })
                .unwrap_or(bytes.len());
            (start, end)
        })
        .collect();

    // ── 4. Parallel processing with work-stealing ──────────────────
    let processed_chunks: Vec<String> = chunks
        .par_iter()
        .map(|&(start, end)| process_chunk_optimized(&bytes[start..end]))
        .collect();

    let out_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(output_path)?;
    let mut writer = BufWriter::with_capacity(WRITE_BUFFER_SIZE, out_file);

    writer.write_all(b"[\n")?;

    let mut first = true;
    for chunk in processed_chunks {
        if !chunk.is_empty() {
            if !first {
                writer.write_all(b",\n")?;
            }
            writer.write_all(chunk.as_bytes())?;
            first = false;
        }
    }

    writer.write_all(b"\n]")?;
    writer.flush()?;
    Ok(())
}

// ──────────────────────────────────────────────────────────────────
// Optimized chunk processor with zero-copy string operations
// ──────────────────────────────────────────────────────────────────
fn process_chunk_optimized(chunk_bytes: &[u8]) -> String {
    // Thread-local reusable buffers
    thread_local! {
        static PARSE_BUF: RefCell<Vec<u8>> =
            RefCell::new(Vec::with_capacity(8192));
        static FNAME_BUF: RefCell<String> =
            RefCell::new(String::with_capacity(512));
        static ITOA_BUF: RefCell<ItoaBuf> =
            RefCell::new(ItoaBuf::new());
    }

    let mut result = String::with_capacity(chunk_bytes.len() * 2);
    let mut first = true;
    let mut pos = 0;

    // Process each line in the chunk
    while pos < chunk_bytes.len() {
        // Find line end using SIMD memchr
        let line_end =
            memchr(b'\n', &chunk_bytes[pos..]).map_or(chunk_bytes.len(), |rel| pos + rel);

        if line_end > pos {
            let line_slice = &chunk_bytes[pos..line_end];

            // Use thread-local buffers to avoid allocations
            PARSE_BUF.with_borrow_mut(|parse_buf| {
                FNAME_BUF.with_borrow_mut(|fname_buf| {
                    ITOA_BUF.with_borrow_mut(|itoa_buf| {
                        // Copy line data for SIMD parser
                        parse_buf.clear();
                        parse_buf.extend_from_slice(line_slice);

                        // SIMD JSON parsing with error handling
                        if let Ok(mut json) = simd_parse::<Value>(parse_buf) {
                            // Fast filename:line_number concatenation
                            if let (Some(fname), Some(line_num)) = (
                                json.get("filename").and_then(Value::as_str),
                                json.get("line_number").and_then(Value::as_u64),
                            ) {
                                fname_buf.clear();
                                fname_buf.push_str(fname);
                                fname_buf.push(':');

                                fname_buf.push_str(itoa_buf.format(line_num));
                                json["filename"] = Value::String(fname_buf.clone());
                            }

                            // Add separator and pretty-print
                            if first {
                                first = false;
                            } else {
                                result.push_str(",\n");
                            }

                            // Direct serialization to avoid intermediate String
                            if let Ok(pretty) = to_string_pretty(&json) {
                                result.push_str(&pretty);
                            }
                        }
                    });
                });
            });
        }

        pos = line_end + 1;
    }

    result
}

use memmap2::Advice;
use rayon::ThreadPoolBuilder;
use simd_json::to_writer_pretty;

// -------------------------------------------------------------------------
// Main entry point - Maximum parallelization
// -------------------------------------------------------------------------
pub fn convert_jsonl_to_pretty_array_parallel<P, Q>(input_path: P, output_path: Q) -> Result<()>
where
    P: AsRef<Path> + Send,
    Q: AsRef<Path> + Send,
{
    // Initialize custom thread pool for optimal CPU utilization
    let thread_pool = ThreadPoolBuilder::new()
        .num_threads(rayon::current_num_threads().max(4))
        .thread_name(|index| format!("jsonl-worker-{index}"))
        .build()
        .map_err(std::io::Error::other)?;

    thread_pool.install(|| {
        parallel_convert_implementation(input_path, output_path, ConversionStrategy::MaxParallel)
    })
}

// -------------------------------------------------------------------------
// Balanced parallel version
// -------------------------------------------------------------------------
pub fn convert_jsonl_to_pretty_array_parallel_max<P, Q>(input_path: P, output_path: Q) -> Result<()>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    parallel_convert_implementation(input_path, output_path, ConversionStrategy::Balanced)
}

// -------------------------------------------------------------------------
// Streaming parallel version for very large files
// -------------------------------------------------------------------------
pub fn convert_jsonl_to_pretty_array_streaming<P, Q>(input_path: P, output_path: Q) -> Result<()>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    parallel_convert_implementation(input_path, output_path, ConversionStrategy::Streaming)
}

// -------------------------------------------------------------------------
// Strategy enum for different parallelization approaches
// -------------------------------------------------------------------------
#[derive(Clone, Copy)]
enum ConversionStrategy {
    MaxParallel, // Maximum parallelization at every step
    Balanced,    // Balance between parallelization and simplicity
    Streaming,   // Process in streaming chunks for large files
}

#[allow(clippy::cast_possible_truncation)]
// -------------------------------------------------------------------------
// Core parallel conversion implementation
// -------------------------------------------------------------------------
fn parallel_convert_implementation<P, Q>(
    input_path: P,
    output_path: Q,
    strategy: ConversionStrategy,
) -> Result<()>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    // ── 1. Parallel file reading and memory mapping ─────────────────
    let (mmap, file_size) = {
        let file: File = File::open(input_path)?;
        let metadata = file.metadata()?;
        let file_size: usize = metadata.len() as usize;

        let mmap: Mmap = unsafe {
            let m: Mmap = Mmap::map(&file)?;

            // Set advice hint directly (non-blocking)
            let _ = m.advise(Advice::Sequential);
            
            m
        };

        (Arc::new(mmap), file_size)
    };

    // ── 2. Parallel line boundary detection ─────────────────────────
    let line_boundaries = find_line_boundaries_parallel(&mmap, file_size);
    let total_lines = line_boundaries.len().saturating_sub(1);

    if total_lines == 0 {
        return write_empty_array(output_path);
    }

    // ── 3. Strategy-specific parallel processing ────────────────────
    let processed_chunks = match strategy {
        ConversionStrategy::MaxParallel => process_lines_max_parallel(&mmap, &line_boundaries),
        ConversionStrategy::Balanced => process_lines_balanced_parallel(&mmap, &line_boundaries),
        ConversionStrategy::Streaming => process_lines_streaming_parallel(&mmap, &line_boundaries),
    };

    // ── 4. Parallel output writing ──────────────────────────────────
    write_json_array_parallel(&processed_chunks, output_path)
}

// -------------------------------------------------------------------------
// Parallel line boundary detection using SIMD
// -------------------------------------------------------------------------
fn find_line_boundaries_parallel(mmap: &[u8], file_size: usize) -> std::vec::Vec<usize> {
    if file_size < 1024 * 1024 {
        // 1MB threshold
        // For small files, use simple sequential approach
        return find_line_boundaries_sequential(mmap);
    }

    // Parallel processing for large files
    let chunk_ranges: Vec<(usize, usize)> = (0..file_size)
        .step_by(BOUNDARY_CHUNK_SIZE)
        .map(|start| {
            let end = (start + BOUNDARY_CHUNK_SIZE).min(file_size);
            (start, end)
        })
        .collect();

    // Find newlines in parallel across chunks
    let chunk_newlines: Vec<Vec<usize>> = chunk_ranges
        .par_iter()
        .map(|&(start, end)| {
            let chunk = &mmap[start..end];
            let mut local_newlines = Vec::new();

            // Use SIMD memchr_iter for fast newline detection
            for pos in memchr_iter(b'\n', chunk) {
                local_newlines.push(start + pos + 1);
            }

            local_newlines
        })
        .collect();

    // Merge results in parallel using reduce
    let mut all_boundaries = vec![0];

    // Parallel flatten and sort
    let all_newlines: Vec<usize> = chunk_newlines
        .par_iter()
        .flat_map(|vec| vec.par_iter().copied())
        .collect();

    let mut all_newlines = all_newlines;
    all_newlines.par_sort_unstable();
    all_boundaries.extend(all_newlines);

    // Ensure we end at file boundary
    if all_boundaries.last() != Some(&file_size) {
        all_boundaries.push(file_size);
    }

    all_boundaries
}

fn find_line_boundaries_sequential(bytes: &[u8]) -> Vec<usize> {
    let mut boundaries = vec![0];
    boundaries.extend(
        memchr_iter(b'\n', bytes)
            .map(|pos| pos + 1)
            .filter(|&pos| pos < bytes.len()),
    );

    if boundaries.last() != Some(&bytes.len()) {
        boundaries.push(bytes.len());
    }

    boundaries
}

// -------------------------------------------------------------------------
// Maximum parallelization strategy
// -------------------------------------------------------------------------
fn process_lines_max_parallel(mmap: &Arc<Mmap>, line_boundaries: &[usize]) -> Vec<String> {
    if line_boundaries.len() < 2 {
        return vec![];
    }

    // Create line ranges for parallel processing
    let line_ranges: Vec<(usize, usize)> =
        line_boundaries.windows(2).map(|w| (w[0], w[1])).collect();

    // Parallel processing with nested parallelism
    let chunk_size = CHUNK_SIZE.max(line_ranges.len() / (rayon::current_num_threads() * 4));

    let processed_chunks: Vec<String> = line_ranges
        .par_chunks(chunk_size)
        .map(|chunk_ranges| {
            // Each thread processes its chunk of lines
            process_line_ranges_parallel(mmap, chunk_ranges)
        })
        .filter(|chunk| !chunk.is_empty())
        .collect();

    processed_chunks
}

// -------------------------------------------------------------------------
// Balanced parallelization strategy
// -------------------------------------------------------------------------
fn process_lines_balanced_parallel(
    mmap: &Arc<Mmap>,
    line_boundaries: &[usize],
) -> Vec<String> {
    if line_boundaries.len() < 2 {
        return vec![];
    }

    let total_lines = line_boundaries.len() - 1;

    if total_lines < MIN_PARALLEL_LINES {
        // Process sequentially for small files
        let line_ranges: Vec<(usize, usize)> =
            line_boundaries.windows(2).map(|w| (w[0], w[1])).collect();
        return vec![process_line_ranges_sequential(mmap, &line_ranges)];
    }

    // Parallel processing with optimal chunk size
    let optimal_chunks = rayon::current_num_threads() * 2;
    let lines_per_chunk = (total_lines / optimal_chunks).max(1);

    let processed_chunks: Vec<String> = line_boundaries
        .par_chunks_exact(lines_per_chunk + 1) // +1 because we need overlapping boundaries
        .chain(line_boundaries.par_chunks(lines_per_chunk + 1).take(1)) // Handle remainder
        .filter_map(|chunk_boundaries| {
            if chunk_boundaries.len() < 2 {
                return None;
            }

            let line_ranges: Vec<(usize, usize)> =
                chunk_boundaries.windows(2).map(|w| (w[0], w[1])).collect();

            let result = process_line_ranges_parallel(mmap, &line_ranges);
            if result.is_empty() {
                None
            } else {
                Some(result)
            }
        })
        .collect();

    processed_chunks
}

// -------------------------------------------------------------------------
// Streaming parallelization strategy
// -------------------------------------------------------------------------
///
/// # Errors
///
/// Parallel Processing Error.
///
fn process_lines_streaming_parallel(
    mmap: &Arc<Mmap>,
    line_boundaries: &[usize],
) -> Vec<String> {
    const STREAM_CHUNK_SIZE: usize = 16384; // Larger chunks for streaming

    let line_ranges: Vec<(usize, usize)> =
        line_boundaries.windows(2).map(|w| (w[0], w[1])).collect();

    // Process in streaming chunks with parallel execution
    let processed_chunks: Vec<String> = line_ranges
        .par_chunks(STREAM_CHUNK_SIZE)
        .map(|chunk_ranges| process_line_ranges_with_streaming(mmap, chunk_ranges))
        .filter(|chunk| !chunk.is_empty())
        .collect();

    processed_chunks
}

// -------------------------------------------------------------------------
// Parallel line processing within chunks
// -------------------------------------------------------------------------
fn process_line_ranges_parallel(mmap: &Arc<Mmap>, line_ranges: &[(usize, usize)]) -> String {
    if line_ranges.is_empty() {
        return String::new();
    }

    // Parallel JSON parsing and transformation
    let parsed_jsons: Vec<Option<Value>> = line_ranges
        .par_iter()
        .map(|&(start, end)| parse_single_line_parallel(mmap, start, end))
        .collect();

    // Parallel pretty-printing
    let pretty_strings: Vec<String> = parsed_jsons
        .par_iter()
        .filter_map(|json_opt| {
            json_opt
                .as_ref()
                .and_then(|json| to_string_pretty(json).ok())
        })
        .collect();

    // Join results
    pretty_strings.join(",\n")
}

fn process_line_ranges_sequential(mmap: &Arc<Mmap>, line_ranges: &[(usize, usize)]) -> String {
    let mut result = String::new();
    let mut first = true;

    for &(start, end) in line_ranges {
        if let Some(json) = parse_single_line_parallel(mmap, start, end)
            && let Ok(pretty) = to_string_pretty(&json)
        {
            if first {
                first = false;
            } else {
                result.push_str(",\n");
            }
            result.push_str(&pretty);
        }
    }

    result
}

fn process_line_ranges_with_streaming(mmap: &Arc<Mmap>, line_ranges: &[(usize, usize)]) -> String {
    // Use parallel iterator with collect for streaming processing
    line_ranges
        .par_iter()
        .filter_map(|&(start, end)| {
            parse_single_line_parallel(mmap, start, end)
                .and_then(|json| to_string_pretty(&json).ok())
        })
        .collect::<Vec<_>>()
        .join(",\n")
}

// -------------------------------------------------------------------------
// Optimized single line parsing with thread-local buffers
// -------------------------------------------------------------------------
fn parse_single_line_parallel(mmap: &Arc<Mmap>, start: usize, end: usize) -> Option<Value> {
    if start >= end {
        return None;
    }

    // Calculate actual line end (trim newline)
    let line_end = if end > start && mmap[end - 1] == b'\n' {
        end - 1
    } else {
        end
    };

    if start >= line_end {
        return None;
    }

    let line_bytes = &mmap[start..line_end];

    // Skip empty lines
    if line_bytes.is_empty() {
        return None;
    }

    // Thread-local buffers for optimal performance
    thread_local! {
        static PARSE_BUF: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(PARSE_BUFFER_SIZE));
        static FILENAME_BUF: RefCell<String> = RefCell::new(String::with_capacity(512));
        static ITOA_BUF: RefCell<ItoaBuf> = RefCell::new(ItoaBuf::new());
    }

    PARSE_BUF.with_borrow_mut(|parse_buf| {
        FILENAME_BUF.with_borrow_mut(|filename_buf| {
            ITOA_BUF.with_borrow_mut(|itoa_buf| {
                // Prepare buffer for parsing
                parse_buf.clear();
                parse_buf.extend_from_slice(line_bytes);

                // Parse JSON with SIMD
                simd_parse::<Value>(parse_buf).map_or_else(|_| None, |mut json| {
                    // Transform filename field with optimized string operations
                    transform_filename_field_optimized(&mut json, filename_buf, itoa_buf);
                    Some(json)
                })
            })
        })
    })
}

// -------------------------------------------------------------------------
// Optimized filename transformation
// -------------------------------------------------------------------------
fn transform_filename_field_optimized(
    json: &mut Value,
    filename_buf: &mut String,
    itoa_buf: &mut ItoaBuf,
) {
    if let Some(obj) = json.as_object_mut() {
        let var_name = (
            obj.get("filename").and_then(Value::as_str),
            obj.get("line_number").and_then(Value::as_u64),
        );

        if let (Some(filename), Some(line_number)) = var_name {
            // Use pre-allocated buffer for string construction
            filename_buf.clear();
            filename_buf.push_str(filename);
            filename_buf.push(':');
            filename_buf.push_str(itoa_buf.format(line_number));

            // Update the JSON object
            obj.insert("filename".to_string(), Value::String(filename_buf.clone()));
        }
    }
}

// -------------------------------------------------------------------------
// Parallel output writing
// -------------------------------------------------------------------------
fn write_json_array_parallel<P>(chunks: &[String], output_path: P) -> Result<()>
where
    P: AsRef<Path>,
{
    // Parallel preparation of output data
    let non_empty_chunks: Vec<&String> = chunks
        .par_iter()
        .filter(|chunk| !chunk.is_empty())
        .collect();

    if non_empty_chunks.is_empty() {
        return write_empty_array(output_path);
    }

    // Parallel size calculation for buffer optimization
    let total_size: usize = non_empty_chunks
        .par_iter()
        .map(|chunk| chunk.len())
        .sum::<usize>() + 
        non_empty_chunks.len() * 2 + // commas and newlines
        4; // [ ] and final newline

    // Write with optimized buffer size
    let buffer_size = WRITE_BUFFER_SIZE.max(total_size / 4);
    let out_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(output_path)?;
    let mut writer = BufWriter::with_capacity(buffer_size, out_file);

    writer.write_all(b"[\n")?;

    // Write chunks with parallel string joining where beneficial
    if non_empty_chunks.len() > 100 {
        // For many chunks, use parallel joining
        let joined = non_empty_chunks
            .par_iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(",\n");
        writer.write_all(joined.as_bytes())?;
    } else {
        // For few chunks, write sequentially
        for (i, chunk) in non_empty_chunks.iter().enumerate() {
            if i > 0 {
                writer.write_all(b",\n")?;
            }
            writer.write_all(chunk.as_bytes())?;
        }
    }

    writer.write_all(b"\n]")?;
    writer.flush()?;
    Ok(())
}

fn write_empty_array<P>(output_path: P) -> Result<()>
where
    P: AsRef<Path>,
{
    let mut file = File::create(output_path)?;
    file.write_all(b"[\n]")?;
    Ok(())
}

// -------------------------------------------------------------------------
// Utility functions for different parallel strategies
// -------------------------------------------------------------------------

/// Get optimal parallelization parameters based on system resources
#[must_use]
pub fn get_parallel_config() -> ParallelConfig {
    let num_cpus = rayon::current_num_threads();
    let available_memory = get_available_memory_mb();

    ParallelConfig {
        num_threads: num_cpus,
        chunk_size: if available_memory > 8192 {
            CHUNK_SIZE * 2
        } else {
            CHUNK_SIZE
        },
        buffer_size: if available_memory > 4096 {
            PARSE_BUFFER_SIZE * 2
        } else {
            PARSE_BUFFER_SIZE
        },
        use_streaming: available_memory < 2048,
    }
}

pub struct ParallelConfig {
    pub num_threads: usize,
    pub chunk_size: usize,
    pub buffer_size: usize,
    pub use_streaming: bool,
}

const fn get_available_memory_mb() -> usize {
    // Simplified memory detection - in real implementation,
    // you'd use system-specific APIs
    8192 // Assume 8GB available
}

// -------------------------------------------------------------------------
// Advanced parallel conversion with custom configuration
// -------------------------------------------------------------------------
///
/// # Errors
///
///
///
pub fn convert_jsonl_with_config<P, Q>(
    input_path: P,
    output_path: Q,
    config: &ParallelConfig,
) -> Result<()>
where
    P: AsRef<Path> + Send,
    Q: AsRef<Path> + Send,
{
    let thread_pool = ThreadPoolBuilder::new()
        .num_threads(config.num_threads)
        .build()
        .map_err(std::io::Error::other)?;

    thread_pool.install(|| {
        let strategy = if config.use_streaming {
            ConversionStrategy::Streaming
        } else {
            ConversionStrategy::MaxParallel
        };

        parallel_convert_implementation(input_path, output_path, strategy)
    })
}

// ------------------------------------------------------------------
// Ultra-fast version: minimal pretty printing
// ------------------------------------------------------------------
pub fn convert_jsonl_to_array_fast<P, Q>(input_path: P, output_path: Q) -> Result<()>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let file = File::open(input_path)?;
    let mmap = unsafe { Mmap::map(&file)? };

    let mut w = BufWriter::with_capacity(BUF_SIZE, File::create(output_path)?);

    w.write_all(b"[\n  ")?;

    let data = &mmap[..];
    let mut start = 0;
    let mut first = true;

    while start < data.len() {
        // Find next newline
        let mut end = start;
        while end < data.len() && data[end] != b'\n' {
            end += 1;
        }

        if end > start {
            let mut line = data[start..end].to_vec();

            if let Ok(mut json) = simd_parse::<Value>(&mut line) {
                if let Some(obj) = json.as_object_mut()
                    && let (Some(Value::String(f)), Some(Value::Number(n))) =
                        (obj.get("filename"), obj.get("line_number"))
                    && let Some(ln) = n.as_u64()
                {
                    obj.insert("filename".to_string(), Value::String(format!("{f}:{ln}")));
                }

                if !first {
                    w.write_all(b",\n  ")?;
                }
                first = false;

                // Use compact representation
                serde_json::to_writer(&mut w, &json)?;
            }
        }

        start = end + 1;
    }

    w.write_all(b"\n]")?;
    w.flush()?;

    Ok(())
}

///
/// # Errors
///
/// Parallel Parsing Error.
///
pub fn finalize_logs() -> Result<()> {
    convert_jsonl_to_pretty_array_parallel_max("logs/app.jsonl", "logs/app.json")
}
