//! Process-owned MSXML DOM used by GTShell's skin and language loader.
//! Documents remain alive while any returned node/list still refers to them.

use quick_xml::{events::Event, Reader};
use std::collections::HashMap;

pub const NODE_ELEMENT: i32 = 1;
pub const NODE_ATTRIBUTE: i32 = 2;
pub const NODE_TEXT: i32 = 3;
pub const NODE_CDATA: i32 = 4;
pub const NODE_PROCESSING_INSTRUCTION: i32 = 7;
pub const NODE_COMMENT: i32 = 8;
pub const NODE_DOCUMENT: i32 = 9;
pub const MAX_XML_BYTES: usize = 8 * 1024 * 1024;
const MAX_NODES: usize = 65536;
const MAX_DEPTH: usize = 256;

#[derive(Debug)]
pub struct DomNode {
    pub node_type: i32,
    pub name: String,
    pub value: String,
    pub children: Vec<usize>,
    pub attributes: Vec<usize>,
    pub parent: Option<usize>,
}

pub struct Document {
    pub nodes: Vec<DomNode>,
    pub root_element: Option<usize>,
}

impl Document {
    pub fn empty() -> Self {
        Self {
            nodes: vec![DomNode {
                node_type: NODE_DOCUMENT,
                name: "#document".into(),
                value: String::new(),
                children: Vec::new(),
                attributes: Vec::new(),
                parent: None,
            }],
            root_element: None,
        }
    }

    fn append(
        &mut self,
        parent: usize,
        node_type: i32,
        name: String,
        value: String,
    ) -> Option<usize> {
        if node_type == NODE_TEXT {
            if let Some(&last) = self.nodes[parent].children.last() {
                if self.nodes[last].node_type == NODE_TEXT {
                    self.nodes[last].value.push_str(&value);
                    return Some(last);
                }
            }
        }
        if self.nodes.len() == MAX_NODES {
            return None;
        }
        let id = self.nodes.len();
        self.nodes.push(DomNode {
            node_type,
            name,
            value,
            children: Vec::new(),
            attributes: Vec::new(),
            parent: Some(parent),
        });
        if node_type == NODE_ATTRIBUTE {
            self.nodes[parent].attributes.push(id);
        } else {
            self.nodes[parent].children.push(id);
        }
        Some(id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComKind {
    Document,
    Node(usize),
    ChildList(usize),
    AttrList(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct ComObject {
    pub doc: u32,
    pub kind: ComKind,
    pub iter_pos: usize,
    pub refs: u32,
    pub preserve_whitespace: bool,
}

#[derive(Default)]
pub struct MsxmlState {
    pub doc_vtable: u32,
    pub node_vtable: u32,
    pub list_vtable: u32,
    pub nnm_vtable: u32,
    pub documents: HashMap<u32, Document>,
    pub objects: HashMap<u32, ComObject>,
    next_document: u32,
}

impl MsxmlState {
    pub fn insert_document(&mut self, document: Document) -> Option<u32> {
        let id = self.next_document.checked_add(1)?;
        self.next_document = id;
        self.documents.insert(id, document);
        Some(id)
    }

    pub fn collect_document(&mut self, id: u32) {
        if !self.objects.values().any(|object| object.doc == id) {
            self.documents.remove(&id);
        }
    }

    pub fn load_document(&mut self, object: u32, xml: &str) -> bool {
        let Some(previous) = self.objects.get(&object).copied() else {
            return false;
        };
        if previous.kind != ComKind::Document {
            return false;
        }
        let Some(document) = parse(xml, previous.preserve_whitespace) else {
            return false;
        };
        let Some(id) = self.insert_document(document) else {
            return false;
        };
        self.objects.get_mut(&object).unwrap().doc = id;
        self.collect_document(previous.doc);
        true
    }

    pub fn doc(&self, id: u32) -> Option<&Document> {
        self.documents.get(&id)
    }

    pub fn node(&self, doc: u32, id: usize) -> Option<&DomNode> {
        self.doc(doc)?.nodes.get(id)
    }

    pub fn text_of(&self, doc: u32, id: usize) -> String {
        let mut text = String::new();
        if let Some(document) = self.doc(doc) {
            collect_text(document, id, &mut text);
        }
        text
    }
}

fn collect_text(document: &Document, id: usize, text: &mut String) {
    let Some(node) = document.nodes.get(id) else {
        return;
    };
    match node.node_type {
        NODE_ATTRIBUTE | NODE_TEXT | NODE_CDATA => text.push_str(&node.value),
        _ => {
            for &child in &node.children {
                collect_text(document, child, text);
            }
        }
    }
}

/// Bounded local XML, without DTDs, external entities or network retrieval.
pub fn parse(xml: &str, preserve_whitespace: bool) -> Option<Document> {
    if xml.len() > MAX_XML_BYTES {
        return None;
    }
    let mut reader = Reader::from_str(xml.trim_start_matches('\u{feff}'));
    reader.config_mut().check_comments = true;
    let mut document = Document::empty();
    let mut stack = vec![0usize];
    loop {
        let event = reader.read_event().ok()?;
        let parent = *stack.last()?;
        match event {
            Event::Start(ref element) | Event::Empty(ref element) => {
                let name = std::str::from_utf8(element.name().as_ref())
                    .ok()?
                    .to_string();
                if name.is_empty() || (parent == 0 && document.root_element.is_some()) {
                    return None;
                }
                let id = document.append(parent, NODE_ELEMENT, name, String::new())?;
                if parent == 0 {
                    document.root_element = Some(id);
                }
                for attribute in element.attributes() {
                    let attribute = attribute.ok()?;
                    let name = std::str::from_utf8(attribute.key.as_ref())
                        .ok()?
                        .to_string();
                    let value = attribute
                        .decode_and_unescape_value(reader.decoder())
                        .ok()?
                        .into_owned();
                    document.append(id, NODE_ATTRIBUTE, name, value)?;
                }
                if matches!(event, Event::Start(_)) {
                    if stack.len() == MAX_DEPTH {
                        return None;
                    }
                    stack.push(id);
                }
            }
            Event::End(_) => {
                if stack.len() == 1 {
                    return None;
                }
                stack.pop();
            }
            Event::Text(text) => {
                let value = text.xml_content().ok()?.into_owned();
                if parent == 0 && !value.trim().is_empty() {
                    return None;
                }
                document.append(parent, NODE_TEXT, "#text".into(), value)?;
            }
            Event::GeneralRef(reference) => {
                if parent == 0 {
                    return None;
                }
                let escaped = format!("&{};", reference.decode().ok()?);
                let value = quick_xml::escape::unescape(&escaped).ok()?.into_owned();
                document.append(parent, NODE_TEXT, "#text".into(), value)?;
            }
            Event::CData(text) => {
                if parent == 0 {
                    return None;
                }
                document.append(
                    parent,
                    NODE_CDATA,
                    "#cdata-section".into(),
                    text.decode().ok()?.into_owned(),
                )?;
            }
            Event::Comment(text) => {
                document.append(
                    parent,
                    NODE_COMMENT,
                    "#comment".into(),
                    text.decode().ok()?.into_owned(),
                )?;
            }
            Event::PI(instruction) => {
                let name = std::str::from_utf8(instruction.target()).ok()?.to_string();
                let value = std::str::from_utf8(instruction.content())
                    .ok()?
                    .trim()
                    .to_string();
                document.append(parent, NODE_PROCESSING_INSTRUCTION, name, value)?;
            }
            Event::Decl(_) => {}
            Event::DocType(_) => return None,
            Event::Eof => break,
        }
    }
    if stack.len() != 1 || document.root_element.is_none() {
        return None;
    }
    if !preserve_whitespace {
        let whitespace: Vec<_> = document
            .nodes
            .iter()
            .map(|node| node.node_type == NODE_TEXT && node.value.trim().is_empty())
            .collect();
        for node in &mut document.nodes {
            node.children.retain(|&id| !whitespace[id]);
        }
    }
    Some(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skin_labels_keep_unicode_entities_and_quoted_brackets() {
        let document = parse("<Skin title=\"Язык &amp; a &gt; b\"><Label>Français &amp; Русский &#x1F642;<![CDATA[ <ok>]]></Label></Skin>", false).unwrap();
        let root = document.root_element.unwrap();
        let attribute = document.nodes[root].attributes[0];
        assert_eq!(document.nodes[attribute].value, "Язык & a > b");
        let mut text = String::new();
        collect_text(&document, root, &mut text);
        assert_eq!(text, "Français & Русский 🙂 <ok>");
    }

    #[test]
    fn malformed_or_external_xml_is_not_reported_as_loaded() {
        for xml in [
            "<a></b>",
            "<a>",
            "<a/><b/>",
            "text<a/>",
            "<a x='1' x='2'/>",
            "<a x=no/>",
            "<a>&unknown;</a>",
            "<!DOCTYPE a SYSTEM 'file:///secret'><a/>",
        ] {
            assert!(parse(xml, false).is_none(), "{xml}");
        }
        assert!(parse(
            &format!("{}{}", "<a>".repeat(MAX_DEPTH), "</a>".repeat(MAX_DEPTH)),
            false
        )
        .is_none());
    }

    #[test]
    fn whitespace_option_changes_only_blank_text_nodes() {
        let compact = parse("<r>\n <a> A &amp; B </a>\n</r>", false).unwrap();
        let preserved = parse("<r>\n <a> A &amp; B </a>\n</r>", true).unwrap();
        assert_eq!(
            compact.nodes[compact.root_element.unwrap()].children.len(),
            1
        );
        assert_eq!(
            preserved.nodes[preserved.root_element.unwrap()]
                .children
                .len(),
            3
        );
    }

    #[test]
    fn reloading_a_document_keeps_retained_old_nodes_alive() {
        let mut state = MsxmlState::default();
        let doc = state
            .insert_document(parse("<Old>original</Old>", false).unwrap())
            .unwrap();
        let node = state.doc(doc).unwrap().root_element.unwrap();
        state.objects.insert(
            100,
            ComObject {
                doc,
                kind: ComKind::Document,
                iter_pos: 0,
                refs: 1,
                preserve_whitespace: false,
            },
        );
        state.objects.insert(
            200,
            ComObject {
                doc,
                kind: ComKind::Node(node),
                iter_pos: 0,
                refs: 1,
                preserve_whitespace: false,
            },
        );
        assert!(state.load_document(100, "<New>replacement</New>"));
        assert_eq!(state.text_of(doc, node), "original");
        assert_ne!(state.objects[&100].doc, doc);
        state.objects.remove(&200);
        state.collect_document(doc);
        assert!(state.doc(doc).is_none());
    }
}
