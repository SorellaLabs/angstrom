// We use jemalloc for performance reasons
#[cfg(all(feature = "jemalloc", unix))]
#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn main() {
    if let Err(err) = angstrom::cli::run() {
        eprintln!("Error: {err:?}");
        std::process::exit(1);
    }
}
