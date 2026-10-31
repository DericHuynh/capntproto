//! Identical measurement hooks are compiled into frozen and candidate workers.
use serde::Serialize;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::Instant;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static BLOCKS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static READS: AtomicU64 = AtomicU64::new(0);
static WRITES: AtomicU64 = AtomicU64::new(0);
static RX: AtomicU64 = AtomicU64::new(0);
static TX: AtomicU64 = AtomicU64::new(0);
pub struct Allocator;
// SAFETY: Allocation ownership/layout and all operations are delegated unchanged
// to System. Counters never allocate and do not inspect allocated memory.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: Forward the caller's valid GlobalAlloc layout.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() && ACTIVE.load(Relaxed) {
            BLOCKS.fetch_add(1, Relaxed);
            BYTES.fetch_add(layout.size() as u64, Relaxed);
        }
        pointer
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: Forward the caller's valid GlobalAlloc layout.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() && ACTIVE.load(Relaxed) {
            BLOCKS.fetch_add(1, Relaxed);
            BYTES.fetch_add(layout.size() as u64, Relaxed);
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: Forward the allocation and its original layout unchanged.
        unsafe { System.dealloc(pointer, layout) };
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: Forward the allocation, layout and requested size unchanged.
        let pointer = unsafe { System.realloc(pointer, layout, size) };
        if !pointer.is_null() && ACTIVE.load(Relaxed) {
            BLOCKS.fetch_add(1, Relaxed);
            BYTES.fetch_add(size as u64, Relaxed);
        }
        pointer
    }
}
extern "C" {
    fn capntproto_callgrind() -> i32;
    fn capntproto_measure_start();
    fn capntproto_measure_stop();
    fn capntproto_cpu_ns() -> u64;
    fn capntproto_cycles_open() -> i32;
    fn capntproto_cycles_start(fd: i32);
    fn capntproto_cycles_stop(fd: i32) -> u64;
}
pub struct Region {
    start: u64,
    wall: Instant,
    cycles: i32,
    cycles_error: Option<String>,
}
#[derive(Serialize)]
pub struct Sample {
    pub wall_ns: u64,
    pub cpu_ns: u64,
    pub cycles: Option<u64>,
    pub cycles_error: Option<String>,
    pub blocks: u64,
    pub bytes: u64,
    pub reads: u64,
    pub writes: u64,
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
    pub callgrind_markers: bool,
}
impl Region {
    pub fn start() -> Self {
        for counter in [&BLOCKS, &BYTES, &READS, &WRITES, &RX, &TX] {
            counter.store(0, Relaxed);
        }
        // SAFETY: These argument-free measurement shims only access their own
        // stack or ask the kernel/Valgrind to start a counter.
        let cycles = unsafe { capntproto_cycles_open() };
        let cycles_error = (cycles < 0).then(|| std::io::Error::last_os_error().to_string());
        // SAFETY: The shims accept the matching open descriptor, including its
        // negative unsupported sentinel, and do not access Rust memory.
        unsafe {
            capntproto_measure_start();
            capntproto_cycles_start(cycles);
        }
        // SAFETY: The argument-free clock shim returns a process counter by value.
        let start = unsafe { capntproto_cpu_ns() };
        let wall = Instant::now();
        ACTIVE.store(true, Relaxed);
        Self {
            start,
            wall,
            cycles,
            cycles_error,
        }
    }
    pub fn stop(self) -> Sample {
        let wall_ns = self
            .wall
            .elapsed()
            .as_nanos()
            .try_into()
            .unwrap_or(u64::MAX);
        ACTIVE.store(false, Relaxed);
        // SAFETY: The descriptor came from the matching open shim, including
        // its documented negative unsupported value. All output is by value.
        let (elapsed, cycles, markers) = unsafe {
            (
                capntproto_cpu_ns().saturating_sub(self.start),
                capntproto_cycles_stop(self.cycles),
                capntproto_callgrind() != 0,
            )
        };
        // SAFETY: This ends the matching profiling region without Rust pointers.
        unsafe {
            capntproto_measure_stop();
        }
        Sample {
            wall_ns,
            cpu_ns: elapsed,
            cycles: (self.cycles >= 0).then_some(cycles),
            cycles_error: self.cycles_error,
            blocks: BLOCKS.load(Relaxed),
            bytes: BYTES.load(Relaxed),
            reads: READS.load(Relaxed),
            writes: WRITES.load(Relaxed),
            received_bytes: RX.load(Relaxed),
            transmitted_bytes: TX.load(Relaxed),
            callgrind_markers: markers,
        }
    }
}
pub fn read(n: usize) {
    if n > 0 && ACTIVE.load(Relaxed) {
        READS.fetch_add(1, Relaxed);
        RX.fetch_add(n as u64, Relaxed);
    }
}
pub fn write(n: usize) {
    if n > 0 && ACTIVE.load(Relaxed) {
        WRITES.fetch_add(1, Relaxed);
        TX.fetch_add(n as u64, Relaxed);
    }
}
