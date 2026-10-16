// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

//! XML owns its selection/value rules. The shared transaction owns installation.
//! All patches are prepared in memory and checked by the XML parser before I/O.
use roxmltree::{Document, Node, ParsingOptions};
use serde_json::Value;
use std::collections::BTreeMap;
use std::ops::Range;

pub(crate) type Namespaces = BTreeMap<String, String>;
#[derive(Debug, Clone, PartialEq, Eq)]
struct Name {
    lexical: String,
    local: String,
    uri: Option<String>,
}
impl Name {
    fn parse(name: &str, ns: &Namespaces) -> Result<Self, String> {
        let parts: Vec<_> = name.split(':').collect();
        fn ncname(s: &str) -> bool {
            let mut chars = s.chars();
            chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        }
        let (local,uri)=match parts.as_slice() {
            [local] if ncname(local) => ((*local).to_string(),None),
            [prefix,local] if ncname(prefix) && ncname(local) => ((*local).to_string(),Some(ns.get(*prefix).ok_or("XML selector uses an undeclared namespace prefix")?.clone())),
            _=>return Err("XML selectors require literal ASCII element names, optionally namespace-prefixed; XPath operators, attributes and indexes are unsupported".into()),
        };
        Ok(Self {
            lexical: name.into(),
            local,
            uri,
        })
    }
    fn matches(&self, node: Node<'_, '_>) -> bool {
        node.is_element()
            && node.tag_name().name() == self.local
            && node.tag_name().namespace().filter(|uri| !uri.is_empty()) == self.uri.as_deref()
    }
    fn same(&self, other: &Self) -> bool {
        self.local == other.local && self.uri == other.uri
    }
    fn element(&self, content: &str, parent: Option<Node<'_, '_>>) -> String {
        let declaration = if let Some((prefix, _)) = self.lexical.split_once(':') {
            let uri = self.uri.as_deref().unwrap();
            if parent.and_then(|n| n.lookup_namespace_uri(Some(prefix))) == Some(uri) {
                String::new()
            } else {
                format!(" xmlns:{prefix}=\"{}\"", escape(uri))
            }
        } else if parent.and_then(|n| n.default_namespace()).is_some() {
            " xmlns=\"\"".into()
        } else {
            String::new()
        };
        format!(
            "<{}{declaration}>{content}</{}>",
            self.lexical, self.lexical
        )
    }
}
fn selector(path: &str, ns: &Namespaces) -> Result<Vec<Name>, String> {
    let path = path
        .strip_prefix('/')
        .ok_or("XML selectors must be absolute element paths")?;
    if path.is_empty() {
        return Err("XML selector cannot be empty".into());
    }
    path.split('/').map(|name| Name::parse(name, ns)).collect()
}
fn parse(source: &str) -> Result<Document<'_>, String> {
    Document::parse_with_options(
        source,
        ParsingOptions {
            allow_dtd: false,
            ..Default::default()
        },
    )
    .map_err(|e| {
        format!(
            "malformed or unsupported XML at {} (document content withheld)",
            e.pos()
        )
    })
}
fn find<'a, 'input>(
    document: &'a Document<'input>,
    path: &[Name],
) -> Result<Option<Node<'a, 'input>>, String> {
    let root = document.root_element();
    if !path[0].matches(root) {
        return Ok(None);
    }
    let mut current = root;
    for name in &path[1..] {
        let mut selected = current.children().filter(|n| name.matches(*n));
        let Some(child) = selected.next() else {
            return Ok(None);
        };
        if selected.next().is_some() {
            return Err("ambiguous XML selector; multiple elements match".into());
        }
        current = child;
    }
    Ok(Some(current))
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn scalar(value: &Value) -> Result<String, String> {
    match value {Value::String(s)=>Ok(s.clone()),Value::Bool(b)=>Ok(b.to_string()),Value::Number(n)=>Ok(n.to_string()),_=>Err("XML values must be strings, booleans, numbers, or an explicit children mapping; null is not deletion".into())}
}
fn scalar_matches(node: Node<'_, '_>, value: &Value) -> Result<bool, String> {
    if node.children().any(|n| n.is_element()) {
        return Err(
            "XML scalar selector has element children; use explicit children syntax".into(),
        );
    }
    let text: String = node
        .children()
        .filter_map(|n| if n.is_text() { n.text() } else { None })
        .collect();
    Ok(match value {
        Value::String(expected) => text == *expected,
        Value::Bool(expected) => match text.trim() {
            "true" | "1" => *expected,
            "false" | "0" => !*expected,
            _ => false,
        },
        Value::Number(expected) => text
            .trim()
            .parse::<serde_json::Number>()
            .is_ok_and(|n| n == *expected),
        _ => return Err("unsupported XML scalar type".into()),
    })
}
#[derive(Debug)]
struct Patch {
    range: Range<usize>,
    text: String,
}
fn apply(source: &str, mut patches: Vec<Patch>) -> Result<String, String> {
    patches.sort_by_key(|p| (p.range.start, p.range.end));
    for pair in patches.windows(2) {
        if pair[0].range.end > pair[1].range.start {
            return Err("overlapping XML edits".into());
        }
    }
    let mut output = source.to_string();
    for patch in patches.into_iter().rev() {
        output.replace_range(patch.range, &patch.text);
    }
    Ok(output)
}
/// Find the opening tag boundary without mistaking `>` inside an attribute.
fn opening(source: &str, node: Node<'_, '_>) -> (usize, bool, String) {
    let start = node.range().start;
    let mut quote = None;
    let mut end = start;
    for (index, ch) in source[start..].char_indices() {
        if matches!(ch, '\'' | '"') {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            }
        }
        if ch == '>' && quote.is_none() {
            end = start + index + 1;
            break;
        }
    }
    let self_closed = source[..end].ends_with("/>");
    let name = source[start + 1..end]
        .split(|c: char| c.is_whitespace() || matches!(c, '/' | '>'))
        .next()
        .unwrap()
        .to_string();
    (end, self_closed, name)
}
fn insert(source: &str, node: Node<'_, '_>, text: String) -> Patch {
    let (end, self_closed, name) = opening(source, node);
    if self_closed {
        let mut replacement = source[node.range().start..end - 2].to_string();
        replacement.push('>');
        replacement.push_str(&text);
        replacement.push_str(&format!("</{name}>"));
        Patch {
            range: node.range(),
            text: replacement,
        }
    } else {
        let closing = source[..node.range().end].rfind("</").unwrap();
        Patch {
            range: closing..closing,
            text,
        }
    }
}
fn scalar_patches(source: &str, node: Node<'_, '_>, value: &Value) -> Result<Vec<Patch>, String> {
    let encoded = escape(&scalar(value)?);
    if scalar_matches(node, value)? {
        return Ok(vec![]);
    }
    let (opening_end, self_closed, _) = opening(source, node);
    if self_closed {
        return Ok(vec![insert(source, node, encoded)]);
    }
    let closing = source[..node.range().end].rfind("</").unwrap();
    // roxmltree coalesces adjacent text/CDATA, whose node range covers only
    // the first lexical token. Derive text spans between preserved comments
    // and processing instructions instead of trusting that coalesced range.
    let mut cursor = opening_end;
    let mut inserted = false;
    let mut patches = Vec::new();
    for boundary in node
        .children()
        .filter(|n| !n.is_text())
        .map(|n| n.range())
        .chain(std::iter::once(closing..closing))
    {
        if cursor < boundary.start {
            patches.push(Patch {
                range: cursor..boundary.start,
                text: if inserted {
                    String::new()
                } else {
                    inserted = true;
                    encoded.clone()
                },
            });
        }
        cursor = boundary.end;
    }
    if !inserted {
        patches.push(Patch {
            range: closing..closing,
            text: encoded,
        });
    }
    Ok(patches)
}

/// Ensure a path by adding only explicitly authorized missing elements.
fn ensure(mut source: String, path: &[Name], create: bool) -> Result<String, String> {
    for depth in 1..=path.len() {
        let document = parse(&source)?;
        if find(&document, &path[..depth])?.is_some() {
            continue;
        }
        if !create {
            return Err(
                "XML selected field or ancestor is missing; set create: true to create it".into(),
            );
        }
        if depth == 1 {
            return Err(
                "XML root does not match selector; refusing to replace document root".into(),
            );
        }
        let parent = find(&document, &path[..depth - 1])?.ok_or("missing XML ancestor")?;
        let patch = insert(&source, parent, path[depth - 1].element("", Some(parent)));
        source = apply(&source, vec![patch])?;
    }
    Ok(source)
}
type ChildLists<'a> = Vec<(Name, &'a Vec<Value>)>;
fn children_spec<'a>(value: &'a Value, ns: &Namespaces) -> Result<Option<ChildLists<'a>>, String> {
    let Value::Object(map) = value else {
        scalar(value)?;
        return Ok(None);
    };
    if map.len() != 1 {
        return Err("XML structured values must contain only children".into());
    }
    let children = map
        .get("children")
        .and_then(Value::as_object)
        .ok_or("XML children must map element names to scalar lists")?;
    if children.is_empty() {
        return Err("XML children requires at least one named list (an empty list removes that name's children)".into());
    }
    let mut result = Vec::new();
    for (name, values) in children {
        let name = Name::parse(name, ns)?;
        if result.iter().any(|(previous, _)| name.same(previous)) {
            return Err("duplicate XML child namespace/name".into());
        }
        let values = values
            .as_array()
            .ok_or("XML repeated children require a list")?;
        for value in values {
            scalar(value)?;
        }
        result.push((name, values));
    }
    Ok(Some(result))
}
fn set_value(
    source: &str,
    path: &[Name],
    value: &Value,
    create: bool,
    ns: &Namespaces,
) -> Result<String, String> {
    let document = parse(source)?;
    let node = find(&document, path)?.ok_or("missing XML selected field")?;
    let Some(children) = children_spec(value, ns)? else {
        return apply(source, scalar_patches(source, node, value)?);
    };
    let mut patches = Vec::new();
    let mut additions = String::new();
    for (name, values) in children {
        let existing: Vec<_> = node.children().filter(|n| name.matches(*n)).collect();
        if values.len() > existing.len() && !create {
            return Err("XML repeated children need new elements; set create: true".into());
        }
        for (index, child) in existing.iter().enumerate() {
            if let Some(value) = values.get(index) {
                patches.extend(scalar_patches(source, *child, value)?);
            } else {
                patches.push(Patch {
                    range: child.range(),
                    text: String::new(),
                });
            }
        }
        let extra = values
            .iter()
            .skip(existing.len())
            .map(|v| Ok(name.element(&escape(&scalar(v)?), Some(node))))
            .collect::<Result<Vec<_>, String>>()?
            .join("");
        if !extra.is_empty() {
            if let Some(last) = existing.last() {
                let end = last.range().end;
                patches.push(Patch {
                    range: end..end,
                    text: extra,
                });
            } else {
                additions.push_str(&extra);
            }
        }
    }
    if !additions.is_empty() {
        patches.push(insert(source, node, additions));
    }
    apply(source, patches)
}

pub(crate) fn edit(
    source: Option<&str>,
    set: &BTreeMap<String, Value>,
    remove: &[String],
    create: bool,
    ns: &Namespaces,
) -> Result<String, String> {
    for (prefix, uri) in ns {
        if prefix == "xmlns"
            || prefix.is_empty()
            || uri.is_empty()
            || (prefix == "xml" && uri != "http://www.w3.org/XML/1998/namespace")
        {
            return Err("invalid XML namespace binding".into());
        }
        Name::parse(&format!("{prefix}:validation"), ns)?;
    }
    let mut paths = Vec::new();
    for (path, value) in set {
        paths.push(selector(path, ns)?);
        children_spec(value, ns)?;
    }
    for path in remove {
        paths.push(selector(path, ns)?);
    }
    for (index, path) in paths.iter().enumerate() {
        for other in paths.iter().skip(index + 1) {
            if path.iter().zip(other).all(|(a, b)| a.same(b)) {
                return Err("conflicting or overlapping XML selectors".into());
            }
        }
    }
    let mut output = match source {
        Some(source) => {
            parse(source)?;
            source.into()
        }
        None if create => {
            let path = paths
                .first()
                .ok_or("creating an XML file requires at least one set operation")?;
            if set.is_empty() {
                return Err("creating an XML file requires a set operation".into());
            }
            path[0].element("", None)
        }
        None => return Err("XML file is missing; set create: true to create it".into()),
    };
    for (path, value) in set {
        let path = selector(path, ns)?;
        output = ensure(output, &path, create)?;
        output = set_value(&output, &path, value, create, ns)?;
    }
    for path in remove {
        let path = selector(path, ns)?;
        if path.len() == 1 {
            return Err("removing the XML document root is unsupported".into());
        }
        let document = parse(&output)?;
        if let Some(node) = find(&document, &path)? {
            output = apply(
                &output,
                vec![Patch {
                    range: node.range(),
                    text: String::new(),
                }],
            )?;
        }
    }
    parse(&output)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn settings(values: Value) -> BTreeMap<String, Value> {
        serde_json::from_value(values).unwrap()
    }
    fn run(source: &str, values: Value) -> String {
        edit(
            Some(source),
            &settings(values),
            &[],
            false,
            &Namespaces::new(),
        )
        .unwrap()
    }

    #[test]
    fn selected_scalars_preserve_unrelated_bytes_comments_instructions_attributes_and_cdata() {
        let source = "<?xml version=\"1.0\"?>\n<?app keep?>\n<!-- top -->\n<Config a='x > y'>\n  <Enabled keep='yes'><!-- inner -->false<?inside retain?></Enabled>\n  <Port> 8096 </Port>\n  <Label><![CDATA[old<&]]></Label>\n  <Unrelated z='2' a=\"1\"> untouched &amp; literal </Unrelated>\n</Config>\n";
        let values = json!({"/Config/Enabled":true,"/Config/Port":8096,"/Config/Label":"new<&"});
        let output = run(source, values.clone());
        assert_eq!(
            output,
            source
                .replace("<!-- inner -->false", "<!-- inner -->true")
                .replace("<![CDATA[old<&]]>", "new&lt;&amp;")
        );
        assert_eq!(run(&output, values), output);
    }
    #[test]
    fn strings_numbers_booleans_empty_elements_and_xml_escaping_converge() {
        let source =
            "<Config><A/><B attr='kept'/><C>1</C><D>4</D><E>left<![CDATA[mid]]>right</E></Config>";
        let values = json!({"/Config/A":"<&\"","/Config/B":"","/Config/C":true,"/Config/D":5,"/Config/E":"joined"});
        let output = run(source, values.clone());
        assert!(output.contains("<A>&lt;&amp;&quot;</A>"));
        assert!(
            output.contains("<B attr='kept'/><C>1</C><D>5</D><E>joined</E>"),
            "{output}"
        );
        assert_eq!(run(&output, values), output);
    }
    #[test]
    fn repeated_children_keep_metadata_other_children_and_explicit_empty_lists() {
        let source = "<Config><KnownProxies marker='keep'>\n<!-- first --><string type='ip'>old</string><?p keep?><Other>keep</Other><string>stale</string>\n</KnownProxies><AppSetting>keep</AppSetting></Config>";
        let values =
            settings(json!({"/Config/KnownProxies":{"children":{"string":["one","two","three"]}}}));
        assert!(edit(Some(source), &values, &[], false, &Namespaces::new()).is_err());
        let output = edit(Some(source), &values, &[], true, &Namespaces::new()).unwrap();
        assert!(output.contains("<!-- first --><string type='ip'>one</string><?p keep?><Other>keep</Other><string>two</string><string>three</string>"));
        assert_eq!(
            edit(Some(&output), &values, &[], true, &Namespaces::new()).unwrap(),
            output
        );
        let empty = run(
            &output,
            json!({"/Config/KnownProxies":{"children":{"string":[]}}}),
        );
        assert!(empty.contains("marker='keep'"));
        assert!(empty.contains("<?p keep?><Other>keep</Other>"));
        assert!(!empty.contains("<string"));
    }
    #[test]
    fn create_is_explicit_for_file_ancestors_fields_and_repeated_values() {
        let values =
            settings(json!({"/Config/Network/Proxies":{"children":{"string":["one","two"]}}}));
        assert!(edit(None, &values, &[], false, &Namespaces::new()).is_err());
        assert!(edit(Some("<Config/>"), &values, &[], false, &Namespaces::new()).is_err());
        let expected = "<Config><Network><Proxies><string>one</string><string>two</string></Proxies></Network></Config>";
        assert_eq!(
            edit(None, &values, &[], true, &Namespaces::new()).unwrap(),
            expected
        );
        assert_eq!(
            edit(Some("<Config/>"), &values, &[], true, &Namespaces::new()).unwrap(),
            expected
        );
        assert!(edit(Some("<Different/>"), &values, &[], true, &Namespaces::new()).is_err());
    }
    #[test]
    fn namespace_selectors_match_uris_and_creation_preserves_namespace_meaning() {
        let namespaces = Namespaces::from([("n".into(), "urn:network".into())]);
        let source =
            "<Root xmlns='urn:network'><Value a='keep'>old</Value><Other>keep</Other></Root>";
        let values = settings(
            json!({"/n:Root/n:Value":"new","/n:Root/n:Proxies":{"children":{"string":["one"]}}}),
        );
        let output = edit(Some(source), &values, &[], true, &namespaces).unwrap();
        assert!(output.contains("<Value a='keep'>new</Value>"));
        let doc = parse(&output).unwrap();
        let proxies = find(&doc, &selector("/n:Root/n:Proxies", &namespaces).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(proxies.tag_name().namespace(), Some("urn:network"));
        assert!(
            proxies
                .first_element_child()
                .unwrap()
                .tag_name()
                .namespace()
                .unwrap_or("")
                .is_empty()
        );
        assert_eq!(
            edit(Some(&output), &values, &[], true, &namespaces).unwrap(),
            output
        );
        let created = edit(None, &values, &[], true, &namespaces).unwrap();
        assert_eq!(
            edit(Some(&created), &values, &[], true, &namespaces).unwrap(),
            created
        );
    }
    #[test]
    fn removals_are_explicit_idempotent_and_cannot_remove_root() {
        let source = "<Config><!-- keep --><Gone a='x'>x</Gone><Other>y</Other></Config>";
        let paths = vec!["/Config/Gone".into(), "/Config/Absent/Child".into()];
        let expected = "<Config><!-- keep --><Other>y</Other></Config>";
        assert_eq!(
            edit(
                Some(source),
                &BTreeMap::new(),
                &paths,
                false,
                &Namespaces::new()
            )
            .unwrap(),
            expected
        );
        assert_eq!(
            edit(
                Some(expected),
                &BTreeMap::new(),
                &paths,
                false,
                &Namespaces::new()
            )
            .unwrap(),
            expected
        );
        assert!(
            edit(
                Some(source),
                &BTreeMap::new(),
                &["/Config".into()],
                false,
                &Namespaces::new()
            )
            .is_err()
        );
    }
    #[test]
    fn ambiguous_overlapping_unsupported_and_malformed_operations_fail_before_writing() {
        for (source, values) in [
            ("<Root><A/><A/></Root>", json!({"/Root/A":true})),
            ("<Root><A><B/></A></Root>", json!({"/Root/A":"x"})),
            ("<Root><A/></Root>", json!({"/Root/A":null})),
            ("<Root><A/></Root>", json!({"/Root/A":["bad"]})),
            (
                "<Root><A/></Root>",
                json!({"/Root/A":{"children":{"string":[{}]}}}),
            ),
            ("<Root><A/></Root>", json!({"/Root/A":{"unknown":[]}})),
            (
                "<Root><A/></Root>",
                json!({"/Root/A":true,"/Root/A/B":false}),
            ),
            ("<Root><A/></Root>", json!({"Root/A":true})),
            ("<Root><A/></Root>", json!({"/Root/A[1]":true})),
            ("<Root><A/></Root>", json!({"/Root/@A":true})),
            ("<Root><A/></Root>", json!({"/Root/*":true})),
            ("<Root><A/></Root>", json!({"/Root/u:A":true})),
            ("<Root><A>bad secret</Root>", json!({"/Root/A":true})),
            (
                "<!DOCTYPE Root [<!ENTITY ext SYSTEM 'file:///etc/passwd'>]><Root><A>&ext;</A></Root>",
                json!({"/Root/A":true}),
            ),
            (
                "<!DOCTYPE Root [<!ENTITY x 'secret'>]><Root><A>&x;</A></Root>",
                json!({"/Root/A":true}),
            ),
        ] {
            let error = edit(
                Some(source),
                &settings(values),
                &[],
                true,
                &Namespaces::new(),
            )
            .unwrap_err();
            assert!(!error.contains("bad secret"));
            assert!(!error.contains("passwd"));
        }
        assert!(
            edit(
                Some("<Root><A/></Root>"),
                &settings(json!({"/Root/A":true})),
                &["/Root/A".into()],
                false,
                &Namespaces::new()
            )
            .is_err()
        );
    }
}
