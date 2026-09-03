//! The kernel is authored as TypeScript and embedded in every compiler build.

pub const KERNEL_SPECIFIER: &str = "alder:kernel";
pub const KERNEL_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/kernel.mjs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_kernel_exports_the_codegen_contract() {
        for symbol in [
            "$equal",
            "$show",
            "$compare",
            "$compareEnum",
            "$hash",
            "$matchFailure",
            "$optionBox",
            "$providerPush",
            "$registerTest",
        ] {
            assert!(KERNEL_JS.contains(&format!("export function {symbol}")));
        }
    }
}
