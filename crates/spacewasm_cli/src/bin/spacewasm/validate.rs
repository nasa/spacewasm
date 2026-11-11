//! `spacewasm validate`: load each module as the flight interpreter would.

use crate::Outcome;
use crate::args::ValidateArgs;
use spacewasm_cli::{Symbols, host_modules_for, validate};

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
        match validate(
            &module,
            args.compiler.options(),
            hosts,
            args.compiler.decoder_limits,
        ) {
            Ok(loaded) => println!(
                "{}: ok, {} bytes, {} IR words in {} code pages",
                path.display(),
                module.len(),
                loaded.ir_words,
                loaded.code_pages
            ),
            Err(err) => {
                all = false;
                eprintln!("{}: {err}", path.display());
            }
        }
    }
    Ok(all)
}
