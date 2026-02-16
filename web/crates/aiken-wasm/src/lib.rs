use wasm_bindgen::prelude::*;

mod vendor;

use aiken_lang::{
    ast::{DataTypeKey, Definition, FunctionAccessKey, ModuleKind, Tracing, TraceLevel},
    builtins,
    expr::TypedExpr,
    gen_uplc::CodeGenerator,
    line_numbers::LineNumbers,
    parser,
    plutus_version::PlutusVersion,
    tipo::TypeInfo,
    utils, IdGenerator,
};
use indexmap::IndexMap;
use std::collections::{BTreeSet, HashMap};
use uplc::ast::{DeBruijn, Program};

const KIND: ModuleKind = ModuleKind::Validator;
const NAME: &str = "play";
const PLUTUS_VERSION: PlutusVersion = PlutusVersion::V3;
const TRACING: Tracing = Tracing::All(TraceLevel::Verbose);

/// Initialize panic hook for better error messages in the browser console.
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

/// Compile Aiken source code to UPLC flat-encoded hex bytes.
///
/// The source must contain at least one `test`. The first test is compiled
/// to a standalone UPLC program (no arguments needed) and returned as hex.
#[wasm_bindgen]
pub fn compile_to_uplc_hex(source: &str) -> Result<String, JsValue> {
    if source.trim().is_empty() {
        return Err(JsValue::from_str("Empty source code"));
    }

    let id_gen = IdGenerator::new();

    // --- Bootstrap built-in types ---
    let mut module_types: HashMap<String, TypeInfo> = HashMap::new();
    module_types.insert("aiken".to_string(), builtins::prelude(&id_gen));
    module_types.insert("aiken/builtin".to_string(), builtins::plutus(&id_gen));

    let mut functions = builtins::prelude_functions(&id_gen, &module_types);
    let mut data_types = builtins::prelude_data_types(&id_gen);
    let mut constants: IndexMap<FunctionAccessKey, TypedExpr> = IndexMap::new();
    let mut module_sources: HashMap<String, (String, LineNumbers)> = HashMap::new();
    let mut dependencies: BTreeSet<String> = BTreeSet::new();

    // --- Load stdlib ---
    setup_dependency(
        "stdlib",
        vendor::stdlib::modules(),
        &vendor::stdlib::MODULES_SEQUENCE[..],
        &id_gen,
        &mut module_types,
        &mut functions,
        &mut constants,
        &mut data_types,
        &mut module_sources,
    )
    .map_err(|e| JsValue::from_str(&e))?;
    dependencies.insert("stdlib".to_string());

    // --- Parse user source ---
    let (mut ast, _extra) = parser::module(source, KIND)
        .map_err(|errs| {
            let msgs: Vec<String> = errs.iter().map(|e| format!("{e}")).collect();
            JsValue::from_str(&format!("Parse error(s):\n{}", msgs.join("\n")))
        })?;
    ast.name = NAME.to_string();

    // --- Type-check ---
    let mut warnings = vec![];
    let package_name = format!("aiken-lang/{NAME}");
    let ast = ast
        .infer(
            &id_gen,
            KIND,
            &package_name,
            &module_types,
            TRACING,
            &mut warnings,
            None,
        )
        .map_err(|e| JsValue::from_str(&format!("Type error: {e}")))?;

    // Register definitions for code generation
    module_sources.insert(
        NAME.to_string(),
        (source.to_string(), LineNumbers::new(source)),
    );
    module_types.insert(NAME.to_string(), ast.type_info.clone());
    ast.register_definitions(&mut functions, &mut constants, &mut data_types);

    // --- Find tests (zero-argument unit tests compile to standalone programs) ---
    let tests: Vec<_> = ast
        .definitions()
        .filter_map(|def| match def {
            Definition::Test(t) if t.arguments.is_empty() => Some(t),
            _ => None,
        })
        .collect();

    if tests.is_empty() {
        return Err(JsValue::from_str(
            "No test found. Aiken source must contain at least one zero-argument `test`.\n\
             Example:\n\
             test my_test() {\n  \
               1 + 1 == 2\n\
             }",
        ));
    }

    // --- Generate UPLC for first test using generate_raw ---
    let mut generator = CodeGenerator::new(
        PLUTUS_VERSION,
        utils::indexmap::as_ref_values(&functions),
        utils::indexmap::as_ref_values(&constants),
        utils::indexmap::as_ref_values(&data_types),
        utils::indexmap::as_str_ref_values(&module_types),
        utils::indexmap::as_str_ref_values(&module_sources),
        TRACING,
    );

    let test = tests[0];
    let program = generator.generate_raw(&test.body, &[], NAME);
    let program: Program<DeBruijn> = program
        .try_into()
        .map_err(|e| JsValue::from_str(&format!("UPLC conversion error: {e:?}")))?;

    // Use to_flat() for raw flat bytes, NOT to_hex() which wraps in CBOR.
    // uplc-turbo's flat::decode() expects raw flat bytes.
    let flat_bytes = program
        .to_flat()
        .map_err(|e| JsValue::from_str(&format!("Flat encoding error: {e:?}")))?;

    Ok(hex::encode(flat_bytes))
}

/// Set up a dependency by parsing and type-checking its modules in order.
fn setup_dependency(
    context: &str,
    modules: HashMap<&str, &str>,
    sequence: &[&str],
    id_gen: &IdGenerator,
    module_types: &mut HashMap<String, TypeInfo>,
    functions: &mut IndexMap<FunctionAccessKey, aiken_lang::ast::TypedFunction>,
    constants: &mut IndexMap<FunctionAccessKey, TypedExpr>,
    data_types: &mut IndexMap<DataTypeKey, aiken_lang::ast::TypedDataType>,
    module_sources: &mut HashMap<String, (String, LineNumbers)>,
) -> Result<(), String> {
    for module_name in sequence {
        let module_src = modules
            .get(module_name)
            .ok_or_else(|| {
                format!("couldn't find sources for '{module_name}' when compiling {context}")
            })?;

        let (mut ast, _extra) = parser::module(module_src, ModuleKind::Lib)
            .map_err(|e| format!("Parse error in {context}/{module_name}: {e:?}"))?;

        ast.name = module_name.to_string();

        let mut warnings = vec![];
        let ast = ast
            .infer(
                id_gen,
                ModuleKind::Lib,
                module_name,
                module_types,
                Tracing::silent(),
                &mut warnings,
                None,
            )
            .map_err(|e| format!("Type error in {context}/{module_name}: {e}"))?;

        ast.register_definitions(functions, constants, data_types);

        module_sources.insert(
            module_name.to_string(),
            (module_src.to_string(), LineNumbers::new(module_src)),
        );

        module_types.insert(module_name.to_string(), ast.type_info);
    }
    Ok(())
}
