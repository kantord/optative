//! Lowers a `.op.mdx` source into a synthetic TSX source string that
//! `optative_script::jsx::transform_source` can eat unchanged.
//!
//! Heading structure defines nesting: prose (and any non-JSX markdown
//! construct — lists, tables, blockquotes, code blocks, etc.) directly under
//! a heading is merged, heading line included, into one `<Context value=...>`
//! wrapping every unit in that section (including subsections). Top-level
//! `import`/`export` ESM statements are hoisted above a synthesized
//! `export default () => (...)` — the author never writes the default export
//! themselves; see [`compile_root`] for the exact shape. Flow-position JSX
//! elements and `{expression}`s are
//! spliced into the output verbatim, by source byte offset. Inline
//! text-position JSX/expressions (mid-sentence, e.g. `a ~<Foo />~ b`) are
//! rejected with a positioned error — not supported yet.

use markdown::mdast::Node;
use markdown::unist::Position;
use markdown::{MdxSignal, ParseOptions};

/// Placeholder path oxc's `SourceType::from_path` uses to infer "parse as
/// TSX" for the synthesized intermediate source. Never surfaced to the user
/// or the evaluated module — [`crate::run_script`] hands the *original*
/// `.op.mdx` path to `optative_script::run_script_with_source` separately,
/// so diagnostics from evaluating the lowered module still point at the
/// real file.
const SYNTHETIC_TSX_PATH: &str = "lowered-module.tsx";

#[derive(Debug, thiserror::Error)]
#[error("{path}:{line}:{column}: {message}")]
pub struct LowerError {
    path: String,
    line: usize,
    column: usize,
    message: String,
}

struct Section {
    /// Heading rank (1..=6); 0 for the implicit root section with no heading.
    depth: u8,
    /// Verbatim source text of the heading line itself, if any.
    heading_text: Option<String>,
    /// Verbatim source text of each direct prose-like node in this section
    /// (paragraphs, lists, tables, blockquotes, code blocks, ...), in order.
    prose_parts: Vec<String>,
    /// Flow JSX elements, flow expressions, and nested subsections, in
    /// document order.
    children: Vec<Child>,
}

enum Child {
    /// Verbatim source text of a flow JSX element or flow `{expression}`.
    Verbatim(String),
    Section(Section),
}

/// Lowers `.op.mdx` source (read from `path`, used only for diagnostics) to
/// a synthetic TSX source string.
pub fn lower_to_tsx(source: &str, path: &str) -> Result<String, LowerError> {
    let mut options = ParseOptions::mdx();
    // markdown-rs calls this at each candidate blank-line/EOF boundary to
    // decide whether the accumulated `import`/`export` statement is done.
    // `esm_statement_is_complete` tracks bracket/string/template-literal
    // nesting so a blank line *inside* e.g. a multi-paragraph prompt
    // template literal doesn't get mistaken for the statement's end.
    options.mdx_esm_parse = Some(Box::new(|value: &str| {
        if esm_statement_is_complete(value) {
            MdxSignal::Ok
        } else {
            MdxSignal::Eof(
                "unterminated string, template literal, or bracket in this import/export statement"
                    .to_string(),
                Box::new("optative-script-mdx".to_string()),
                Box::new("unterminated-esm-statement".to_string()),
            )
        }
    }));

    let tree = markdown::to_mdast(source, &options).map_err(|e| {
        let (line, column) = point_from_message(&e);
        LowerError {
            path: path.to_string(),
            line,
            column,
            message: e.to_string(),
        }
    })?;

    let root_children: &[Node] = tree.children().map(Vec::as_slice).unwrap_or(&[]);
    let (root_section, esm_statements) = build_root_section(source, path, root_children)?;

    let mut body = String::new();
    compile_root(&root_section, &mut body);

    if body.contains("<Context")
        && !esm_statements
            .iter()
            .any(|s| binds_identifier(s, "Context"))
    {
        return Err(LowerError {
            path: path.to_string(),
            line: 1,
            column: 1,
            message: "this document has headings/prose, which are lowered to `<Context>` \
                      elements, but no `Context` import was found — add e.g. `import { Context } \
                      from 'esto'` at the top of the file"
                .to_string(),
        });
    }

    let mut out = String::new();
    for stmt in &esm_statements {
        out.push_str(stmt);
        out.push('\n');
    }
    out.push_str("export default () => (\n  ");
    out.push_str(&body);
    out.push_str("\n);\n");
    Ok(out)
}

/// Renders the root section's body. When the root itself has a heading or
/// direct prose, [`compile_section`] already produces exactly one
/// `<Context>...</Context>` element — no further wrapping needed. Otherwise
/// (the common case: everything lives under real headings) the root has no
/// single containing element, so its children are returned as a plain array
/// — the same array-of-elements shape `.op.tsx` scripts already use for
/// multiple top-level units (e.g. `[<Thing name="widget" />]`). This avoids
/// ever needing JSX fragment (`<>...</>`) shorthand, which would require the
/// author to additionally import `Fragment` themselves for no real benefit.
fn compile_root(section: &Section, out: &mut String) {
    let has_context = section.heading_text.is_some() || !section.prose_parts.is_empty();
    if has_context {
        compile_section(section, out);
        return;
    }
    let items: Vec<String> = section
        .children
        .iter()
        .map(|child| {
            let mut item = String::new();
            match child {
                Child::Verbatim(text) => item.push_str(text),
                Child::Section(sub) => compile_section(sub, &mut item),
            }
            item
        })
        .collect();
    out.push('[');
    out.push_str(&items.join(",\n"));
    out.push(']');
}

/// The path oxc's source-type sniffing should see for a lowered `.op.mdx`
/// module — see [`SYNTHETIC_TSX_PATH`].
pub fn synthetic_tsx_path() -> &'static str {
    SYNTHETIC_TSX_PATH
}

fn build_root_section(
    source: &str,
    path: &str,
    root_children: &[Node],
) -> Result<(Section, Vec<String>), LowerError> {
    let mut esm_statements = Vec::new();
    let mut stack: Vec<Section> = vec![Section {
        depth: 0,
        heading_text: None,
        prose_parts: vec![],
        children: vec![],
    }];

    for node in root_children {
        match node {
            Node::MdxjsEsm(esm) => {
                esm_statements.push(slice(source, path, esm.position.as_ref())?);
            }
            Node::Heading(heading) => {
                if let Some(pos) = find_inline_mdx(node) {
                    return Err(mk_error(
                        path,
                        pos,
                        "inline JSX/expressions inside headings are not supported yet — use a flow-position element on its own line instead",
                    ));
                }
                let heading_text = slice(source, path, node.position())?;
                let depth = heading.depth;
                while stack.len() > 1 && stack.last().unwrap().depth >= depth {
                    let finished = stack.pop().unwrap();
                    stack
                        .last_mut()
                        .unwrap()
                        .children
                        .push(Child::Section(finished));
                }
                stack.push(Section {
                    depth,
                    heading_text: Some(heading_text),
                    prose_parts: vec![],
                    children: vec![],
                });
            }
            Node::MdxJsxFlowElement(_) | Node::MdxFlowExpression(_) => {
                let text = slice(source, path, node.position())?;
                stack
                    .last_mut()
                    .unwrap()
                    .children
                    .push(Child::Verbatim(text));
            }
            _ => {
                let text = slice(source, path, node.position())?;
                if looks_like_forgotten_export(&text) {
                    let pos = node
                        .position()
                        .expect("slice() already required a position");
                    return Err(mk_error(
                        path,
                        pos,
                        "this looks like a JS/TS statement, not prose — did you forget `export`? \
                         .op.mdx only recognizes top-level `import`/`export` statements as code; \
                         anything else is treated as prose and folded into the surrounding context text",
                    ));
                }
                if let Some(pos) = find_inline_mdx(node) {
                    return Err(mk_error(
                        path,
                        pos,
                        "inline JSX/expressions inside prose are not supported yet — use a flow-position element on its own line instead",
                    ));
                }
                stack.last_mut().unwrap().prose_parts.push(text);
            }
        }
    }

    while stack.len() > 1 {
        let finished = stack.pop().unwrap();
        stack
            .last_mut()
            .unwrap()
            .children
            .push(Child::Section(finished));
    }

    Ok((stack.pop().unwrap(), esm_statements))
}

fn compile_section(section: &Section, out: &mut String) {
    let has_context = section.heading_text.is_some() || !section.prose_parts.is_empty();
    if has_context {
        let mut merged = String::new();
        if let Some(heading) = &section.heading_text {
            merged.push_str(heading);
        }
        for part in &section.prose_parts {
            if !merged.is_empty() {
                merged.push_str("\n\n");
            }
            merged.push_str(part);
        }
        let literal = serde_json::to_string(&merged).expect("String always serializes to JSON");
        out.push_str("<Context value={");
        out.push_str(&literal);
        out.push_str("}>");
    }
    for child in &section.children {
        match child {
            Child::Verbatim(text) => out.push_str(text),
            Child::Section(sub) => compile_section(sub, out),
        }
    }
    if has_context {
        out.push_str("</Context>");
    }
}

/// A single open bracket kind, tracked so its specific closing character is
/// the only thing that can pop it back off the stack.
#[derive(PartialEq)]
enum Bracket {
    Paren,
    Square,
    Brace,
}

/// One "mode" `esm_statement_is_complete` can be scanning in.
enum ScanMode {
    /// Ordinary code: `(`/`[`/`{` push a matching [`Bracket`]; a quote or
    /// backtick switches into the corresponding string/template mode.
    Code(Vec<Bracket>),
    SingleQuote,
    DoubleQuote,
    /// Inside a template literal's literal-text portions. `${` switches to
    /// [`ScanMode::Code`] (pushed onto `resume`) until its matching `}`.
    Template,
}

/// Whether `text` — the accumulated source of a candidate top-level
/// `import`/`export` statement — is free of any unclosed string, template
/// literal, or bracket. markdown-rs calls this at each blank-line/EOF
/// boundary; returning "not complete" tells it to keep collecting past the
/// blank line instead of ending the statement there. Without this, a blank
/// line inside e.g. a multi-paragraph `prompt\`...\`` template literal would
/// be mistaken for the end of the statement.
///
/// This is a bracket/string/template depth scanner, not a real JS/TS
/// parser — it can't catch genuine syntax errors (oxc's `transform_source`
/// does that downstream), it only tracks enough nesting to know whether a
/// blank line is safe to treat as a statement boundary.
fn esm_statement_is_complete(text: &str) -> bool {
    let mut mode = ScanMode::Code(Vec::new());
    let mut resume: Vec<ScanMode> = Vec::new();
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match &mut mode {
            ScanMode::Code(brackets) => match c {
                '(' => brackets.push(Bracket::Paren),
                '[' => brackets.push(Bracket::Square),
                '{' => brackets.push(Bracket::Brace),
                ')' if brackets.last() == Some(&Bracket::Paren) => {
                    brackets.pop();
                }
                ']' if brackets.last() == Some(&Bracket::Square) => {
                    brackets.pop();
                }
                '}' => {
                    if brackets.last() == Some(&Bracket::Brace) {
                        brackets.pop();
                    } else if brackets.is_empty() {
                        // Closes a `${` opened from Template — pop back to it.
                        if let Some(popped) = resume.pop() {
                            mode = popped;
                        }
                    }
                }
                '\'' => brackets_push_mode(&mut mode, &mut resume, ScanMode::SingleQuote),
                '"' => brackets_push_mode(&mut mode, &mut resume, ScanMode::DoubleQuote),
                '`' => brackets_push_mode(&mut mode, &mut resume, ScanMode::Template),
                '/' if chars.peek() == Some(&'/') => {
                    for c in chars.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
                '/' if chars.peek() == Some(&'*') => {
                    chars.next();
                    let mut prev = '\0';
                    for c in chars.by_ref() {
                        if prev == '*' && c == '/' {
                            break;
                        }
                        prev = c;
                    }
                }
                _ => {}
            },
            ScanMode::SingleQuote | ScanMode::DoubleQuote => {
                if c == '\\' {
                    chars.next();
                } else if (matches!(mode, ScanMode::SingleQuote) && c == '\'')
                    || (matches!(mode, ScanMode::DoubleQuote) && c == '"')
                {
                    mode = resume.pop().unwrap_or(ScanMode::Code(Vec::new()));
                }
            }
            ScanMode::Template => {
                if c == '\\' {
                    chars.next();
                } else if c == '`' {
                    mode = resume.pop().unwrap_or(ScanMode::Code(Vec::new()));
                } else if c == '$' && chars.peek() == Some(&'{') {
                    chars.next();
                    resume.push(ScanMode::Template);
                    mode = ScanMode::Code(Vec::new());
                }
            }
        }
    }

    matches!(mode, ScanMode::Code(brackets) if brackets.is_empty())
}

/// Pushes `next` as the active mode, remembering `mode` (moved out via a
/// placeholder swap) so a later closing delimiter can restore it.
fn brackets_push_mode(mode: &mut ScanMode, resume: &mut Vec<ScanMode>, next: ScanMode) {
    let previous = std::mem::replace(mode, next);
    resume.push(previous);
}

/// A crude, deliberately conservative heuristic for "this prose node is
/// actually a JS/TS statement the author forgot to prefix with `export`".
/// Only `.op.mdx`'s recognized ESM statements (`import ...` / `export ...`
/// at flow position) become real bindings — a bare `const X = ...` or
/// `interface Foo { ... }` silently becomes inert prose text instead, which
/// is either invisible (nothing later references the binding) or surfaces
/// as a confusing downstream `ReferenceError`/inline-JSX-rejection with no
/// hint of the real cause. Catches the common cases without flagging
/// ordinary prose that happens to start with a matching English word (e.g.
/// "type of documentation...") by requiring `keyword IDENTIFIER` followed
/// immediately by `=`, `(`, `{`, or `:` — a shape ordinary prose essentially
/// never takes.
fn looks_like_forgotten_export(text: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "const",
        "let",
        "var",
        "function",
        "async",
        "class",
        "interface",
        "type",
        "enum",
    ];
    let trimmed = text.trim_start();
    let Some(keyword) = KEYWORDS.iter().find(|kw| {
        trimmed
            .strip_prefix(**kw)
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_whitespace()))
    }) else {
        return false;
    };
    let after_keyword = trimmed[keyword.len()..].trim_start();
    let Some(identifier_end) = after_keyword.find(|c: char| !is_js_identifier_char(c)) else {
        return false;
    };
    if identifier_end == 0 {
        return false;
    }
    let after_identifier = after_keyword[identifier_end..].trim_start();
    after_identifier.starts_with(['=', '(', '{', ':'])
}

fn is_js_identifier_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Whether `esm_text` binds the identifier `name` into scope — i.e. `name`
/// appears as a whole word that isn't immediately renamed away via
/// `name as OtherName` (which binds `OtherName`, not `name`). Covers named
/// imports (`import { name } from ...`), default imports (`import name from
/// ...`), rename-*into* `name` (`import { X as name } from ...`), and local
/// declarations (`const name = ...`) — anything that would make a bare
/// reference to `name` resolve. Deliberately lenient: a false negative here
/// (an unusual binding form it doesn't recognize) just means an extra,
/// harmless "please import `Context`" prompt; it can't produce a false
/// crash since real usage of `name` is untouched either way.
fn binds_identifier(esm_text: &str, name: &str) -> bool {
    let mut search_from = 0;
    while let Some(rel) = esm_text[search_from..].find(name) {
        let start = search_from + rel;
        let end = start + name.len();
        let before_ok = esm_text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !is_js_identifier_char(c));
        let after_ok = esm_text[end..]
            .chars()
            .next()
            .is_none_or(|c| !is_js_identifier_char(c));
        if before_ok && after_ok {
            let renamed_away = esm_text[end..]
                .trim_start()
                .strip_prefix("as")
                .is_some_and(|rest| rest.starts_with(|c: char| c.is_whitespace()));
            if !renamed_away {
                return true;
            }
        }
        search_from = end;
    }
    false
}

/// Recursively searches `node`'s descendants for an inline (text-position)
/// JSX element or expression — not supported in v1 (see module docs).
fn find_inline_mdx(node: &Node) -> Option<&Position> {
    if matches!(
        node,
        Node::MdxJsxTextElement(_) | Node::MdxTextExpression(_)
    ) {
        return node.position();
    }
    node.children()?.iter().find_map(find_inline_mdx)
}

fn slice(source: &str, path: &str, position: Option<&Position>) -> Result<String, LowerError> {
    let pos = position.ok_or_else(|| LowerError {
        path: path.to_string(),
        line: 1,
        column: 1,
        message: "internal error: markdown-rs node is missing position info".to_string(),
    })?;
    Ok(source[pos.start.offset..pos.end.offset].to_string())
}

fn mk_error(path: &str, position: &Position, message: &str) -> LowerError {
    LowerError {
        path: path.to_string(),
        line: position.start.line,
        column: position.start.column,
        message: message.to_string(),
    }
}

fn point_from_message(message: &markdown::message::Message) -> (usize, usize) {
    match message.place.as_deref() {
        Some(markdown::message::Place::Position(p)) => (p.start.line, p.start.column),
        Some(markdown::message::Place::Point(p)) => (p.line, p.column),
        None => (1, 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMPORT: &str = "import { h, Context } from 'esto'\n\n";

    #[test]
    fn single_paragraph_becomes_root_context() {
        let src = format!("{IMPORT}Hello world.\n");
        let out = lower_to_tsx(&src, "test.op.mdx").unwrap();
        assert!(
            out.contains(r#"<Context value={"Hello world."}>"#),
            "expected root-level Context, got: {out}"
        );
        assert!(out.contains("export default () => ("));
    }

    #[test]
    fn heading_and_prose_merge_into_one_context_including_heading_line() {
        let src = format!("{IMPORT}## Package: core\n\nPublished, zero-dep.\n");
        let out = lower_to_tsx(&src, "test.op.mdx").unwrap();
        assert!(
            out.contains(r###"value={"## Package: core\n\nPublished, zero-dep."}"###),
            "expected heading + prose merged verbatim (heading included), got: {out}"
        );
    }

    #[test]
    fn prose_before_and_after_an_element_merges_into_one_context() {
        // Position of prose within a section doesn't matter — text before
        // and after a flow element still merges into a single Context.
        let src = format!(
            "{IMPORT}## Package: core\n\nPublished, zero-dep.\n\n<Doc name=\"foo\" />\n\nKeep examples short.\n"
        );
        let out = lower_to_tsx(&src, "test.op.mdx").unwrap();
        assert!(
            out.contains(
                r###"value={"## Package: core\n\nPublished, zero-dep.\n\nKeep examples short."}"###
            ),
            "expected merged context spanning both sides of the element, got: {out}"
        );
        // Exactly one Context wraps the section (not one per paragraph).
        assert_eq!(out.matches("<Context").count(), 1);
        assert!(out.contains(r#"<Doc name="foo" />"#));
    }

    #[test]
    fn nested_headings_produce_nested_contexts() {
        let src = format!("{IMPORT}# Repo\n\nTop prose.\n\n## Package\n\nInner prose.\n");
        let out = lower_to_tsx(&src, "test.op.mdx").unwrap();
        assert_eq!(out.matches("<Context").count(), 2);
        assert_eq!(out.matches("</Context>").count(), 2);
        let repo_idx = out.find("# Repo").unwrap();
        let package_idx = out.find("## Package").unwrap();
        let repo_close_idx = out.rfind("</Context>").unwrap();
        assert!(
            repo_idx < package_idx && package_idx < repo_close_idx,
            "## Package section should be nested inside # Repo's Context, got: {out}"
        );
    }

    #[test]
    fn heading_level_skip_does_not_add_empty_wrapper_level() {
        // # then ### with no ## in between — no error, no empty middle level.
        let src = format!("{IMPORT}# Repo\n\n### Deep\n\nDeep prose.\n");
        let out = lower_to_tsx(&src, "test.op.mdx").unwrap();
        assert_eq!(out.matches("<Context").count(), 2);
    }

    #[test]
    fn returning_to_a_shallower_heading_closes_the_deeper_section() {
        let src = format!("{IMPORT}# A\n\n### B\n\nb text\n\n## C\n\nc text\n");
        let out = lower_to_tsx(&src, "test.op.mdx").unwrap();
        // # A, ### B, ## C — three sections total.
        assert_eq!(out.matches("<Context").count(), 3);
        let a_idx = out.find("# A").unwrap();
        let b_idx = out.find("### B").unwrap();
        let c_idx = out.find("## C").unwrap();
        let b_close = out[b_idx..].find("</Context>").unwrap() + b_idx;
        assert!(
            a_idx < b_idx && b_idx < b_close && b_close < c_idx,
            "## C should open after ### B's Context has closed (sibling of ### B under # A), got: {out}"
        );
    }

    #[test]
    fn top_level_esm_is_hoisted_above_the_default_export() {
        let src = "import { h, Context } from 'esto'\n\nSome prose.\n";
        let out = lower_to_tsx(src, "test.op.mdx").unwrap();
        let import_idx = out.find("import { h, Context } from 'esto'").unwrap();
        let default_idx = out.find("export default () => (").unwrap();
        assert!(
            import_idx < default_idx,
            "import should be hoisted above the synthesized default export, got: {out}"
        );
    }

    #[test]
    fn list_folds_into_enclosing_prose_verbatim() {
        let src = format!("{IMPORT}## Notes\n\n- one\n- two\n");
        let out = lower_to_tsx(&src, "test.op.mdx").unwrap();
        // Merged text is emitted as a JSON-escaped JS string literal, so a
        // real newline in the source shows up as a literal `\n` escape here.
        assert!(
            out.contains("- one\\n- two"),
            "expected list text folded into context verbatim, got: {out}"
        );
        assert!(out.contains("## Notes"));
    }

    #[test]
    fn flow_expression_is_spliced_verbatim() {
        let src = format!(
            "{IMPORT}## Items\n\n{}\n",
            "{['a', 'b'].map((n) => <Item name={n} />)}"
        );
        let out = lower_to_tsx(&src, "test.op.mdx").unwrap();
        assert!(
            out.contains("{['a', 'b'].map((n) => <Item name={n} />)}"),
            "expected flow expression spliced verbatim, got: {out}"
        );
    }

    #[test]
    fn inline_jsx_in_prose_is_rejected_with_position() {
        let src = "Intro.\n\nHello <Foo /> world.\n";
        let err = lower_to_tsx(src, "test.op.mdx").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("test.op.mdx:3:"),
            "expected error positioned on line 3, got: {msg}"
        );
        assert!(msg.contains("not supported yet"), "got: {msg}");
    }

    #[test]
    fn inline_expression_in_prose_is_rejected() {
        let src = "Value is {x} here.\n";
        let err = lower_to_tsx(src, "test.op.mdx").unwrap_err();
        assert!(err.to_string().contains("not supported yet"));
    }

    #[test]
    fn section_with_no_prose_and_no_heading_omits_context_wrapper() {
        // A bare flow element with nothing before it (no root prose) should
        // not be wrapped in a needless empty Context.
        let out = lower_to_tsx("<Doc name=\"foo\" />\n", "test.op.mdx").unwrap();
        assert!(!out.contains("<Context"), "got: {out}");
        assert!(out.contains(r#"<Doc name="foo" />"#));
    }

    #[test]
    fn blank_line_inside_template_literal_does_not_end_the_esm_statement() {
        // Regression test: a naive "any blank line ends the statement"
        // parser would truncate this mid-template-literal, breaking the
        // primary use case — multi-paragraph prompt template literals.
        let src = "import { h, unit, prompt } from 'esto'\n\nexport const Doc = unit({\n  key: (i) => i.name,\n  value: () => 'v',\n  observe: () => [],\n  enter: (i) => prompt`Paragraph one.\n\nParagraph two.`,\n})\n\n<Doc name=\"foo\" />\n";
        let out = lower_to_tsx(src, "test.op.mdx").unwrap();
        assert!(
            out.contains("Paragraph one.\n\nParagraph two.`"),
            "template literal should survive intact with its internal blank line, got: {out}"
        );
        assert!(out.contains(r#"<Doc name="foo" />"#));
    }

    #[test]
    fn unclosed_template_literal_at_eof_is_a_real_error() {
        let src = "import { h, prompt } from 'esto'\n\nexport const x = prompt`unterminated\n";
        let err = lower_to_tsx(src, "test.op.mdx").unwrap_err();
        // Should surface as a positioned parse error, not silently misparse.
        assert!(err.to_string().contains("test.op.mdx:"), "got: {err}");
    }

    #[test]
    fn forgotten_export_on_single_line_const_is_rejected() {
        let src = "import { h } from 'esto'\n\nconst OWNER = 'kantord'\n";
        let err = lower_to_tsx(src, "test.op.mdx").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("did you forget `export`?"), "got: {msg}");
        assert!(msg.contains("test.op.mdx:3:"), "got: {msg}");
    }

    #[test]
    fn forgotten_export_on_multiline_brace_statement_is_rejected_clearly() {
        // Without the forgotten-export check, this used to be misdetected as
        // an unsupported inline JSX/expression — a misleading error that
        // hides the real cause.
        let src =
            "import { h, unit } from 'esto'\n\nconst Doc = unit({\n  key: (i) => i.name,\n})\n";
        let err = lower_to_tsx(src, "test.op.mdx").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("did you forget `export`?"), "got: {msg}");
        assert!(
            !msg.contains("inline JSX"),
            "should report the real cause, not the inline-JSX misdetection, got: {msg}"
        );
    }

    #[test]
    fn prose_starting_with_a_keyword_like_english_word_is_not_flagged() {
        // "type of documentation" etc. must not trip the forgotten-export
        // heuristic — it requires `keyword IDENTIFIER` immediately followed
        // by =, (, {, or :, a shape ordinary prose doesn't take.
        for body in [
            "## Notes\n\ntype of documentation matters here.\n",
            "## Notes\n\nclass of problems we solve.\n",
            "## Notes\n\nfunction as intended.\n",
            "## Notes\n\nlet me explain this.\n",
        ] {
            let src = format!("{IMPORT}{body}");
            let out = lower_to_tsx(&src, "test.op.mdx");
            assert!(out.is_ok(), "should not flag ordinary prose, got: {out:?}");
        }
    }

    #[test]
    fn esm_statement_is_complete_tracks_nested_brackets_strings_and_comments() {
        assert!(esm_statement_is_complete("import { h } from 'esto'"));
        assert!(!esm_statement_is_complete("export const x = ("));
        assert!(!esm_statement_is_complete("export const x = 'unterminated"));
        assert!(!esm_statement_is_complete("export const x = `unterminated"));
        assert!(esm_statement_is_complete(
            "export const x = `a ${obj.foo({a:1})}!`"
        ));
        assert!(esm_statement_is_complete(
            "export const x = 1 // a comment with a { brace"
        ));
        assert!(esm_statement_is_complete(
            "export const x = 1 /* a { brace in a block comment */"
        ));
    }

    #[test]
    fn missing_context_import_is_a_clear_error_not_a_runtime_reference_error() {
        let src = "import { h } from 'esto'\n\nHello world.\n";
        let err = lower_to_tsx(src, "test.op.mdx").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("no `Context` import was found"), "got: {msg}");
        assert!(msg.contains("test.op.mdx:1:1"), "got: {msg}");
    }

    #[test]
    fn document_with_no_headings_or_prose_does_not_need_context_import() {
        // Nothing lowers to <Context> here, so no import is required at all.
        let src = "import { h } from 'esto'\n\n<Doc name=\"foo\" />\n";
        assert!(lower_to_tsx(src, "test.op.mdx").is_ok());
    }

    #[test]
    fn renamed_context_import_still_satisfies_the_check() {
        // `X as Context` binds the name `Context` — should count.
        let src = "import { Ctx as Context } from 'esto'\n\nHello world.\n";
        assert!(lower_to_tsx(src, "test.op.mdx").is_ok());
    }

    #[test]
    fn context_renamed_away_does_not_satisfy_the_check() {
        // `Context as X` does NOT bind the name `Context` — the generated
        // code references the literal identifier `Context`, which wouldn't
        // exist under this import.
        let src = "import { Context as Ctx } from 'esto'\n\nHello world.\n";
        let err = lower_to_tsx(src, "test.op.mdx").unwrap_err();
        assert!(err.to_string().contains("no `Context` import was found"));
    }

    #[test]
    fn binds_identifier_examples() {
        assert!(binds_identifier(
            "import { h, Context } from 'esto'",
            "Context"
        ));
        assert!(binds_identifier(
            "import { Context } from 'esto'",
            "Context"
        ));
        assert!(binds_identifier(
            "import { X as Context } from 'esto'",
            "Context"
        ));
        assert!(!binds_identifier(
            "import { Context as X } from 'esto'",
            "Context"
        ));
        assert!(!binds_identifier("import { h } from 'esto'", "Context"));
        assert!(!binds_identifier(
            "import { ContextThing } from 'esto'",
            "Context"
        ));
    }
}
