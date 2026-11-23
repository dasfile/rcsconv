//! CLI layer: argument parsing and report printing.
//!
//! The interface rules mirror the Python reference (`tools/rcs_cli.py`, README):
//! the first argument is the input (file, directory or mask), the output name is
//! derived (`<name>.rcs`), an existing file is not overwritten without `-f`.
//!
//! The logic (flag parsing, input walking, conversion, atomic write) lives in the
//! `rcs-conv` crate — the GUI (`rcsconv-gui`) uses the same one.

use rcs_conv::{convert_one, expand_sources, has_suffix, NoProgress, Options, Status};

/// Walk the inputs and build `.rcs` files; return the exit code.
pub fn run(command: &str, args: &[String]) -> i32 {
    let las = command == "las2rcs";
    let options = match Options::parse(args, las) {
        Ok(options) => options,
        Err(err) => {
            eprintln!("rcsconv {command}: {}", err.0);
            return 2;
        }
    };
    if let Err(message) = options.validate() {
        eprintln!("rcsconv {command}: {message}");
        return 2;
    }
    let build_options = match options.build_options() {
        Ok(value) => value,
        Err(message) => {
            eprintln!("rcsconv {command}: {message}");
            return 2;
        }
    };

    let sources = expand_sources(&options.sources, Options::suffixes(las), options.recursive);
    if sources.is_empty() {
        eprintln!("rcsconv {command}: no input files found");
        return 2;
    }
    if let Some(wrong) = sources.iter().find(|path| has_suffix(path, ".rcs")) {
        eprintln!(
            "rcsconv {command}: {} is a .rcs file, but this converter reads {}",
            wrong.display(),
            if las { ".las/.laz" } else { ".pts" }
        );
        return 2;
    }

    let threads = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1)
        .min(sources.len())
        .max(1);

    let mut results: Vec<Option<rcs_conv::Conversion>> = Vec::with_capacity(sources.len());
    results.resize_with(sources.len(), || None);

    if threads > 1 {
        let slots = std::sync::Mutex::new(results);
        let next = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| {
                    // Split the memory budget between workers so a batch cannot
                    // exhaust RAM: each conversion gets budget / threads.
                    let mut local = options.clone();
                    if !local.in_memory && local.memory_budget_mb > 0 {
                        local.memory_budget_mb =
                            (local.memory_budget_mb / threads as u32).max(64);
                    }
                    loop {
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if index >= sources.len() {
                            break;
                        }
                        let result = convert_one(
                            &sources[index],
                            &local,
                            &build_options,
                            las,
                            &NoProgress,
                        );
                        if let Ok(mut guard) = slots.lock() {
                            guard[index] = Some(result);
                        }
                    }
                });
            }
        });
        results = slots.into_inner().unwrap_or_else(|error| error.into_inner());
    } else {
        for (index, source) in sources.iter().enumerate() {
            results[index] = Some(convert_one(source, &options, &build_options, las, &NoProgress));
        }
    }

    let mut written = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    for result in results.into_iter().flatten() {
        match result.status {
            Status::Written => {
                written += 1;
                println!("{}", result.message());
            }
            Status::Skipped => {
                skipped += 1;
                println!("{}", result.message());
            }
            Status::Failed | Status::Cancelled => {
                failed += 1;
                eprintln!("{}", result.message());
            }
        }
    }
    if sources.len() > 1 {
        println!("total: {written} written, {skipped} skipped, {failed} failed");
    }
    if failed == 0 {
        0
    } else {
        1
    }
}
