use spacewasm::{
    CodeBuilder, CompilerOptions, ExportDesc,
    InterpreterResult, InterpreterRunner, ModuleRef, PageAllocator, Ref,
    WasmRef,
};

use spacewasm_util::{FileStream, RustSystemAllocator};

spacewasm::global_allocator!(
    PageAllocator<16>,
    PageAllocator::new(&RustSystemAllocator {}, 8192)
);

const MAX_PAGES: usize = 256;
const MAX_CONTROL_FRAMES: usize = 64;
const MAX_STACK_DEPTH: usize = 256;

fn main() {
    let path = std::env::args().nth(1).unwrap();

    let mut code_builder = CodeBuilder::<MAX_PAGES>::default();
    let mut store = spacewasm::Store::new(1, []).unwrap();

    let file = std::fs::File::open(path).expect("failed to open file");
    let mut file_stream = FileStream::new(file);

    let module = spacewasm::Module::new::<MAX_PAGES, MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
            "main",
            &mut file_stream,
            &mut store,
            &mut code_builder,
            spacewasm::Rc::new(RustSystemAllocator)
                .unwrap()
                .into_wasm_memory_allocator(),
            CompilerOptions::default(),
        )
        .expect("failed to parse wasm module");

    let (text, _) = code_builder.finish().unwrap();

    let mut state = store.allocate(1024).unwrap();
    match state.initialize_module(module, &text, usize::MAX) {
        InterpreterResult::Finished => {}
        InterpreterResult::OutOfFuel => panic!("insufficient fuel for initialization"),
        InterpreterResult::Trap(t) => panic!("trap during initialization {t:?}"),
        InterpreterResult::ReaderError(e) => panic!("ir reader error {e:?}"),
        InterpreterResult::Pause => panic!("pause during init"),
    }

    let module: &spacewasm::Module = state.store.modules().last().unwrap();

    let fi = {
        let f = module.exports.iter().find(|f| &f.name == "_start").unwrap();
        let ExportDesc::Func(fi) = f.desc else {panic!()};
        fi
    };

    let Ref::Module(fi) = module.get_func_ref(fi).unwrap() else {panic!()};

    state.invoke(WasmRef {module: ModuleRef(0),index: fi,},&[]).unwrap();

    let interpreter = spacewasm::Interpreter::default();

    let mut result = InterpreterResult::OutOfFuel;
    while result == InterpreterResult::OutOfFuel {
        result = interpreter.run(&text, &mut state, usize::MAX)
    }

    let InterpreterResult::Finished = result else {
        panic!("interpreter failed: {:?}", result)
    };
}
