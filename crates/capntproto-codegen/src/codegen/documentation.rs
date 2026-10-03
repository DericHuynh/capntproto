//! Source-info lookup and include!-compatible Rustdoc emission.
use super::{line, FormattedText, GeneratorContext};
use capnp::schema_capnp::code_generator_request;
use std::collections::HashMap;

struct Entry {
    node: String,
    members: Vec<String>,
}

pub(super) struct Documentation(HashMap<u64, Entry>);

impl Documentation {
    pub(super) fn new(request: code_generator_request::Reader<'_>) -> capnp::Result<Self> {
        let mut entries = HashMap::new();
        for info in request.get_source_info()? {
            let entry = Entry {
                node: attribute(info.get_doc_comment()?)?,
                members: info
                    .get_members()?
                    .iter()
                    .map(|m| attribute(m.get_doc_comment()?))
                    .collect::<capnp::Result<_>>()?,
            };
            if entries.insert(info.get_id(), entry).is_some() {
                return Err(capnp::Error::failed(format!(
                    "duplicate source-info for node {}",
                    info.get_id()
                )));
            }
        }
        Ok(Self(entries))
    }

    pub(super) fn attribute(&self, id: u64, member: Option<usize>) -> &str {
        let Some(info) = self.0.get(&id) else {
            return "";
        };
        if let Some(index) = member {
            // Source info is optional; older or partial requests may omit members.
            info.members.get(index).map_or("", String::as_str)
        } else {
            &info.node
        }
    }

    pub(super) fn formatted(&self, id: u64, member: Option<usize>) -> FormattedText {
        let attribute = self.attribute(id, member);
        if attribute.is_empty() {
            FormattedText::Branch(vec![])
        } else {
            line(attribute.trim_end())
        }
    }
}

fn attribute(text: capnp::text::Reader<'_>) -> capnp::Result<String> {
    use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};
    let text = text.to_str()?;
    if text.is_empty() {
        return Ok(String::new());
    }
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let mut changed = false;
    let mut code = false;
    let mut links = 0;
    let mut events = Vec::new();
    for event in pulldown_cmark::TextMergeStream::new(Parser::new_ext(text, options)) {
        let event = match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                code = true;
                let untagged = match &kind {
                    CodeBlockKind::Indented => true,
                    CodeBlockKind::Fenced(language) => language.is_empty(),
                };
                if untagged {
                    changed = true;
                    Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced("text".into())))
                } else {
                    Event::Start(Tag::CodeBlock(kind))
                }
            }
            Event::End(pulldown_cmark::TagEnd::CodeBlock) => {
                code = false;
                event
            }
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => {
                links += 1;
                event
            }
            Event::End(pulldown_cmark::TagEnd::Link | pulldown_cmark::TagEnd::Image) => {
                links -= 1;
                event
            }
            Event::Text(text) if !code && links == 0 => {
                changed |= autolink(text, &mut events);
                continue;
            }
            event => event,
        };
        events.push(event);
    }
    let mut normalized = String::new();
    let text = if changed {
        // Untagged schema examples are Cap'n Proto/pseudocode, not Rust doctests.
        // Let CommonMark handle nested lists/quotes and embedded fence delimiters.
        let count = pulldown_cmark_to_cmark::calculate_code_block_token_count(&events).unwrap_or(4);
        pulldown_cmark_to_cmark::cmark_with_options(
            events.iter(),
            &mut normalized,
            pulldown_cmark_to_cmark::Options {
                code_block_token_count: count,
                ..Default::default()
            },
        )
        .map_err(|e| capnp::Error::failed(format!("render schema documentation: {e}")))?;
        normalized.as_str()
    } else {
        text
    };
    // A Rust string literal preserves Markdown and cannot turn quotes,
    // carriage returns or comment delimiters into generated Rust code.
    Ok(format!("#[doc = {text:?}]\n"))
}

// Link ordinary HTTP(S) URLs from language-neutral schema prose. Code and
// existing link/image labels never reach this function. Keep punctuation outside
// the link without losing it from the rendered documentation.
fn autolink<'a>(
    text: pulldown_cmark::CowStr<'a>,
    output: &mut Vec<pulldown_cmark::Event<'a>>,
) -> bool {
    use pulldown_cmark::{Event, LinkType, Tag, TagEnd};
    let mut cursor = 0;
    let mut changed = false;
    while let Some(start) = [
        text[cursor..].find("https://"),
        text[cursor..].find("http://"),
    ]
    .into_iter()
    .flatten()
    .min()
    .map(|i| cursor + i)
    {
        let mut end = text[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '\"' | '\''))
            .map_or(text.len(), |i| start + i);
        let mut parens = 0i64;
        let mut brackets = 0i64;
        for byte in text[start..end].bytes() {
            match byte {
                b'(' => parens += 1,
                b')' => parens -= 1,
                b'[' => brackets += 1,
                b']' => brackets -= 1,
                _ => (),
            }
        }
        while let Some(last) = text[start..end].chars().last() {
            let unmatched = (last == ')' && parens < 0) || (last == ']' && brackets < 0);
            if !matches!(last, '.' | ',' | ';' | ':' | '!' | '?') && !unmatched {
                break;
            }
            if last == ')' {
                parens += 1;
            }
            if last == ']' {
                brackets += 1;
            }
            end -= last.len_utf8();
        }
        // Even a malformed scheme must advance the scan without losing text.
        if end
            <= start
                + if text[start..].starts_with("https:") {
                    8
                } else {
                    7
                }
        {
            end = (start + 7).min(text.len());
            output.push(Event::Text(text[cursor..end].to_owned().into()));
            cursor = end;
            continue;
        }
        output.push(Event::Text(text[cursor..start].to_owned().into()));
        let url = &text[start..end];
        output.push(Event::Start(Tag::Link {
            link_type: LinkType::Autolink,
            dest_url: url.to_owned().into(),
            title: "".into(),
            id: "".into(),
        }));
        output.push(Event::Text(url.to_owned().into()));
        output.push(Event::End(TagEnd::Link));
        cursor = end;
        changed = true;
    }
    output.push(Event::Text(text[cursor..].to_owned().into()));
    changed
}

pub(super) fn attach(doc: &FormattedText, item: FormattedText) -> FormattedText {
    if matches!(&item, FormattedText::Branch(items) if items.is_empty()) {
        item
    } else {
        FormattedText::Branch(vec![doc.clone(), item])
    }
}

pub(super) fn file(ctx: &GeneratorContext, id: u64) -> capnp::Result<FormattedText> {
    let doc = ctx.documentation.formatted(id, None);
    if matches!(&doc, FormattedText::Branch(items) if items.is_empty()) {
        return Ok(doc);
    }
    // Inner attributes are rejected when a generated file is included with
    // include!(). Give file documentation its own public Rustdoc page instead.
    // Avoid collisions with actual declarations, including $Rust.name overrides.
    let node = ctx.node_map[&id];
    let names = node
        .get_nested_nodes()?
        .iter()
        .map(|n| ctx.get_last_name(n.get_id()))
        .collect::<capnp::Result<std::collections::HashSet<_>>>()?;
    let mut name = "schema_documentation".to_owned();
    let mut suffix = 0;
    while names.contains(name.as_str()) {
        suffix += 1;
        name = format!("schema_documentation_{suffix}");
    }
    Ok(attach(&doc, line(format!("pub mod {name} {{}}"))))
}
