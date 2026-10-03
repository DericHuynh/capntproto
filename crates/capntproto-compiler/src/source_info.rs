//! Documentation attachment and statement ranges, independent of diagnostics.
use crate::syntax::{Span, Token, TokenKind};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub(crate) struct Info {
    pub span: Span,
    pub comment: Option<Arc<str>>,
}

#[derive(Default)]
struct Sequence {
    active: Option<usize>, // First token / statement start byte.
    opening_comment: Option<Arc<str>>,
}

// KJ's docComment parser accepts comments following the terminator, with
// at most one initial line ending and no blank lines between comment lines.
// It strips '#' and at most one following space, preserving other bytes.
fn comment(text: &str, after: usize) -> (Option<Arc<str>>, usize) {
    let bytes = text.as_bytes();
    let line_space = |pos: &mut usize| {
        while bytes
            .get(*pos)
            .is_some_and(|b| matches!(b, b' ' | b'\t' | 11 | 12))
        {
            *pos += 1;
        }
    };
    let mut pos = after;
    line_space(&mut pos);
    if bytes.get(pos) == Some(&b'\r') {
        pos += 1;
        if bytes.get(pos) == Some(&b'\n') {
            pos += 1;
        }
    } else if bytes.get(pos) == Some(&b'\n') {
        pos += 1;
    }
    let mut end = after;
    let mut result = String::new();
    let mut found = false;
    loop {
        line_space(&mut pos);
        if bytes.get(pos) != Some(&b'#') {
            break;
        }
        found = true;
        pos += 1;
        if bytes.get(pos) == Some(&b' ') {
            pos += 1;
        }
        let start = pos;
        while pos < bytes.len() && bytes[pos] != b'\n' {
            pos += 1;
        }
        result.push_str(&text[start..pos]);
        result.push('\n');
        if pos < bytes.len() {
            pos += 1;
        }
        end = pos;
    }
    (found.then(|| Arc::from(result)), end)
}

pub(crate) fn index(text: &str, tokens: &[Token]) -> BTreeMap<usize, Info> {
    let mut result = BTreeMap::new();
    let mut stack = vec![Sequence::default()];
    for token in tokens {
        let nested = stack.len() > 1;
        let scope = stack.last_mut().unwrap();
        match token.kind {
            TokenKind::End => break,
            TokenKind::Punct('}') if nested => {
                stack.pop();
                let scope = stack.last_mut().unwrap();
                let (late, end) = comment(text, token.span.end);
                if let Some(start) = scope.active.take() {
                    result.insert(
                        start,
                        Info {
                            span: Span { start, end },
                            comment: scope.opening_comment.take().or(late),
                        },
                    );
                }
            }
            _ => {
                scope.active.get_or_insert(token.span.start);
                match token.kind {
                    TokenKind::Punct('{') => {
                        scope.opening_comment = comment(text, token.span.end).0;
                        stack.push(Sequence::default());
                    }
                    TokenKind::Punct(';') => {
                        let (comment, end) = comment(text, token.span.end);
                        let start = scope.active.take().unwrap();
                        result.insert(
                            start,
                            Info {
                                span: Span { start, end },
                                comment,
                            },
                        );
                    }
                    _ => (),
                }
            }
        }
    }
    result
}

pub(crate) fn emit(
    graph: &crate::source::Graph,
    compiled: &crate::resolve::Compilation,
    request: capnp::schema_capnp::code_generator_request::Builder<'_>,
) -> Result<(), crate::Diagnostic> {
    use crate::syntax::NodeKind;
    let mut output = request.init_source_info(compiled.selected.len() as u32);
    let mut remaining = 16 * 1024 * 1024usize;
    for (position, &index) in compiled.selected.iter().enumerate() {
        let node = &graph.nodes[index];
        let mut out = output.reborrow().get(position as u32);
        out.set_id(node.id);
        out.set_start_byte(node.info.span.start as u32);
        out.set_end_byte(node.info.span.end as u32);
        let mut charge = |info: &Info| -> Result<(), crate::Diagnostic> {
            let bytes = info.comment.as_ref().map_or(0, |s| s.len() + 1);
            remaining = remaining.checked_sub(bytes).ok_or_else(|| {
                graph
                    .source(index)
                    .error(node.span, "expanded documentation limit exceeded (16 MiB)")
            })?;
            Ok(())
        };
        charge(&node.info)?;
        if let Some(comment) = &node.info.comment {
            out.set_doc_comment(comment.as_ref());
        }
        let members: Vec<_> = match &node.kind {
            // The C++ struct translator leaves empty members null, whereas
            // enum and interface translators initialize their empty lists.
            NodeKind::Struct(fields) if !fields.is_empty() => {
                fields.iter().map(|f| &f.info).collect()
            }
            NodeKind::Enum(enumerants) => enumerants.iter().map(|e| &e.info).collect(),
            NodeKind::Interface { methods, .. } => methods.iter().map(|m| &m.info).collect(),
            _ => continue,
        };
        let mut members_out = out.init_members(members.len() as u32);
        for (i, info) in members.into_iter().enumerate() {
            charge(info)?;
            let mut member = members_out.reborrow().get(i as u32);
            member.set_start_byte(info.span.start as u32);
            member.set_end_byte(info.span.end as u32);
            if let Some(comment) = &info.comment {
                member.set_doc_comment(comment.as_ref());
            }
        }
    }
    Ok(())
}
