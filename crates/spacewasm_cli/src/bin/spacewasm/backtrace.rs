//! `spacewasm backtrace`: PCs or module offsets, resolved to source.

use crate::Outcome;
use crate::args::BacktraceArgs;
use spacewasm_cli::{Resolved, Symbols, host_modules_for, ir_map, resolve_offsets, resolve_pcs};

pub fn run(args: &BacktraceArgs) -> Outcome {
    let symbols =
        Symbols::read(&args.symbols).map_err(|err| format!("{}: {err}", args.symbols.display()))?;

    if let Some(module) = &args.module {
        let flown = std::fs::read(module).map_err(|err| format!("{}: {err}", module.display()))?;
        if !symbols.describes(&flown) {
            return Err(format!(
                "{} is not the module {} was built for; the lines it named would be wrong",
                module.display(),
                args.symbols.display()
            )
            .into());
        }
    }

    let resolved = match args.pc.is_empty() {
        true => resolve_offsets(&symbols, &args.offset)?,
        false => {
            let pcs = args
                .pc
                .iter()
                .map(|&pc| {
                    pc.checked_sub(args.base)
                        .ok_or_else(|| format!("PC {pc:#x} is below --base {:#x}", args.base))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let hosts = host_modules_for(symbols.module())?;
            let map = ir_map(
                symbols.module(),
                args.compiler.options(),
                hosts,
                args.compiler.decoder_limits,
            )?;
            resolve_pcs(&symbols, &map, &pcs)?
        }
    };

    print!("{}", render(&resolved));
    Ok(resolved.iter().all(|entry| !entry.frames.is_empty()))
}

/// One block per entry; numbered when there is more than one, as a backtrace.
fn render(resolved: &[Resolved]) -> String {
    let mut out = String::new();
    let numbered = resolved.len() > 1;
    for (depth, entry) in resolved.iter().enumerate() {
        if numbered {
            out.push_str(&format!("#{depth} "));
        }
        match entry.pc {
            Some(pc) => out.push_str(&format!("pc {pc:#x}, offset {:#x}", entry.offset)),
            None => out.push_str(&format!("offset {:#x}", entry.offset)),
        }
        if entry.frames.is_empty() {
            out.push_str(": no source recorded\n");
            continue;
        }
        out.push_str(":\n");
        for frame in &entry.frames {
            out.push_str(&format!(
                "  {}\n",
                frame.function.as_deref().unwrap_or("??")
            ));
            if let Some(location) = &frame.location {
                out.push_str(&format!("      at {location}\n"));
            }
        }
    }
    out
}
