use std::path::Path;

use oxc_allocator::Allocator;
use oxc_codegen::Codegen;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{JsxOptions, JsxRuntime, TransformOptions, Transformer};

pub fn transform_jsx(source: &str) -> String {
    let allocator = Allocator::default();
    let source_type = SourceType::jsx();
    let ret = Parser::new(&allocator, source, source_type).parse();
    let mut program = ret.program;

    let scoping = SemanticBuilder::new()
        .with_excess_capacity(2.0)
        .build(&program)
        .semantic
        .into_scoping();

    let options = TransformOptions {
        jsx: JsxOptions {
            runtime: JsxRuntime::Classic,
            pragma: Some("h".to_string()),
            pragma_frag: Some("Fragment".to_string()),
            ..JsxOptions::enable()
        },
        ..TransformOptions::default()
    };

    Transformer::new(&allocator, Path::new("input.jsx"), &options)
        .build_with_scoping(scoping, &mut program);

    Codegen::new().build(&program).code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_closing_element() {
        let out = transform_jsx(r#"<Foo bar="baz" />"#);
        assert!(out.contains("h("), "expected h() call, got: {out}");
        assert!(out.contains("\"Foo\"") || out.contains("Foo"), "expected Foo, got: {out}");
    }

    #[test]
    fn fragment() {
        let out = transform_jsx(r#"<>hello</>  "#);
        assert!(out.contains("Fragment"), "expected Fragment, got: {out}");
    }

    #[test]
    fn pragma_is_h_not_jsx() {
        let out = transform_jsx(r#"<A />"#);
        assert!(!out.contains("_jsx"), "should not contain _jsx, got: {out}");
        assert!(out.contains("h("), "expected h(), got: {out}");
    }
}
