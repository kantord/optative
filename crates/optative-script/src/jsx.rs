use std::path::Path;

use oxc_allocator::Allocator;
use oxc_codegen::Codegen;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{JsxOptions, JsxRuntime, TransformOptions, Transformer};

/// Transform a JSX/TSX/TS source file to plain JS.
/// `path` is used to infer the source type (.jsx/.tsx/.ts/.mjs etc.); it does not need to exist.
pub fn transform_source(source: &str, path: &str) -> String {
    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::jsx());
    let has_jsx = source_type.is_jsx();

    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, source_type).parse();
    let mut program = ret.program;

    let scoping = SemanticBuilder::new()
        .with_excess_capacity(2.0)
        .build(&program)
        .semantic
        .into_scoping();

    let options = TransformOptions {
        jsx: if has_jsx {
            JsxOptions {
                runtime: JsxRuntime::Classic,
                pragma: Some("h".to_string()),
                pragma_frag: Some("Fragment".to_string()),
                ..JsxOptions::enable()
            }
        } else {
            JsxOptions::default()
        },
        ..TransformOptions::default()
    };

    Transformer::new(&allocator, Path::new(path), &options)
        .build_with_scoping(scoping, &mut program);

    Codegen::new().build(&program).code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_closing_element() {
        let out = transform_source(r#"<Foo bar="baz" />"#, "test.jsx");
        assert!(out.contains("h("), "expected h() call, got: {out}");
        assert!(out.contains("\"Foo\"") || out.contains("Foo"), "expected Foo, got: {out}");
    }

    #[test]
    fn fragment() {
        let out = transform_source(r#"<>hello</>  "#, "test.jsx");
        assert!(out.contains("Fragment"), "expected Fragment, got: {out}");
    }

    #[test]
    fn pragma_is_h_not_jsx() {
        let out = transform_source(r#"<A />"#, "test.jsx");
        assert!(!out.contains("_jsx"), "should not contain _jsx, got: {out}");
        assert!(out.contains("h("), "expected h(), got: {out}");
    }

    #[test]
    fn tsx_strips_type_annotations() {
        let out = transform_source(
            r#"const x: number = 42; const el = <Foo bar="baz" />;"#,
            "input.tsx",
        );
        assert!(!out.contains(": number"), "type annotation should be stripped");
        assert!(out.contains("h("), "JSX should be transformed to h()");
    }

    #[test]
    fn ts_strips_type_annotations_no_jsx() {
        let out = transform_source(
            r#"const x: number = 42; export default x;"#,
            "input.ts",
        );
        assert!(!out.contains(": number"), "type annotation should be stripped");
    }

    #[test]
    fn op_tsx_extension_recognized() {
        let out = transform_source(r#"const n: number = 1; const el = <A />;"#, "foo.op.tsx");
        assert!(!out.contains(": number"), "type annotation should be stripped");
        assert!(out.contains("h("), "JSX should be transformed");
    }
}
