//! `spacewasm validate`: load each module as the flight interpreter would, and check that
//! what it allocates fits the flight heap.

use crate::Outcome;
use crate::args::ValidateArgs;
use spacewasm_tools::{Heap, HeapPages, Symbols, host_modules_for, validate};

pub fn run(args: &ValidateArgs) -> Outcome {
    let mut all = true;
    for path in &args.modules {
        let bytes = std::fs::read(path).map_err(|err| format!("{}: {err}", path.display()))?;
        // A symbol file stands for the module it carries.
        let module = match path.extension().is_some_and(|extension| extension == "sym") {
            true => Symbols::parse(&bytes)
                .map_err(|err| format!("{}: {err}", path.display()))?
                .module()
                .to_vec(),
            false => bytes,
        };

        let hosts =
            host_modules_for(&module).map_err(|err| format!("{}: {err}", path.display()))?;
        let loaded = match validate(
            &module,
            args.compiler.options(),
            hosts,
            args.compiler.decoder_limits,
        ) {
            Ok(loaded) => loaded,
            Err(err) => {
                all = false;
                eprintln!("{}: {err}", path.display());
                continue;
            }
        };
        let summary = format!(
            "{} bytes, {} IR words in {} code pages",
            module.len(),
            loaded.ir_words,
            loaded.code_pages
        );
        // The binary's allocator records, so a load that succeeded has a heap.
        let heap = loaded
            .heap
            .expect("the global allocator is a `HeapRecorder`");
        let pages = match heap.on_pages(args.heap_page_size) {
            Ok(pages) => pages,
            Err(err) => {
                all = false;
                eprintln!("{}: {summary}, but {err}", path.display());
                continue;
            }
        };

        let fits = pages.pages <= args.heap_pages;
        let heap_summary = format!(
            "{} heap bytes in {} of {} {}-byte pages",
            pages.bytes(),
            pages.pages,
            args.heap_pages,
            args.heap_page_size
        );
        match fits {
            true => println!("{}: ok, {summary}, {heap_summary}", path.display()),
            false => {
                all = false;
                eprintln!(
                    "{}: {summary}, but needs {heap_summary}. Raise \
                     `GLOBAL_ALLOCATOR_MAX_PAGES` (`--heap-pages`) or shrink the module",
                    path.display()
                );
            }
        }
        if args.verbose {
            print_sections(&heap, &pages);
        }
    }
    Ok(all)
}

/// What each section allocated, as laid out on the flight heap's pages.
fn print_sections(heap: &Heap, pages: &HeapPages) {
    let width = heap
        .sections
        .iter()
        .map(|section| section.name.len())
        .chain(["section".len()])
        .max()
        .unwrap_or(0);
    println!(
        "  {:<width$}  {:>10}  {:>10}  {:>8}  {:>11}",
        "section", "size", "heap", "padding", "allocations"
    );
    for (section, used) in heap.sections.iter().zip(&pages.sections) {
        println!(
            "  {:<width$}  {:>10}  {:>10}  {:>8}  {:>11}",
            section.name,
            section.size,
            used.bytes,
            used.padding,
            section.allocations.len()
        );
    }
    println!(
        "  {:<width$}  {:>10}  {:>10}  {:>8}  {:>11}",
        "total",
        heap.sections
            .iter()
            .map(|section| section.size)
            .sum::<usize>(),
        pages.bytes(),
        pages.padding(),
        heap.sections
            .iter()
            .map(|section| section.allocations.len())
            .sum::<usize>()
    );
}
