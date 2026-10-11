use super::{Region, StyleRule, Theme};
use crate::{CadError, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

/// The portable presentation style contract, version 1. Vendor-specific additions
/// must use `renderer-` names rather than silently changing portable semantics.
pub const SUPPORTED_STYLE_PROPERTIES: &[&str] = &[
    "color",
    "background",
    "accent-color",
    "font-size",
    "font-family",
    "font-weight",
    "font-variant-numeric",
    "line-height",
    "text-align",
    "gap",
    "row-gap",
    "column-gap",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border",
    "border-color",
    "border-width",
    "border-radius",
    "opacity",
    "display",
    "visibility",
    "width",
    "height",
    "min-width",
    "max-width",
    "min-height",
    "max-height",
    "flex-grow",
    "flex-direction",
    "align-items",
    "justify-content",
    "position",
    "top",
    "right",
    "bottom",
    "left",
    "overflow",
    "cursor",
    "selection-color",
    "preview-color",
    "comparison-color",
    "selection-opacity",
    "ghost-opacity",
    "stroke-width",
    "size",
    "hit-size",
    "hit-width",
    "hit-height",
    "icon-size",
    "button-width",
    "button-height",
    "handle-size",
    "min-target-size",
    "construction-opacity",
    "reference-curve-color",
    "explode-line-color",
    "inspection-first-color",
    "inspection-second-color",
    "drawing-active",
    "drawing-item",
    "drawing-preview",
    "drawing-section",
    "drawing-view",
    "shape",
    "anchor",
    "z-index",
];
const INHERITED: &[&str] = &[
    "color",
    "font-size",
    "font-family",
    "font-weight",
    "font-variant-numeric",
    "line-height",
    "cursor",
    "accent-color",
];
#[derive(Clone, Debug, Default)]
pub struct StyleNode {
    pub id: String,
    pub role: String,
    pub classes: Vec<String>,
    pub attributes: BTreeMap<String, String>,
    pub states: Vec<String>,
}
#[derive(Clone, Debug, Default)]
pub struct StyleTarget {
    pub node: StyleNode,
    /// Outermost ancestor first; the immediate parent is last.
    pub ancestors: Vec<StyleNode>,
}
impl StyleTarget {
    pub fn child(&self, id: &str, role: &str, classes: &[String], state: &str) -> Self {
        let mut ancestors = self.ancestors.clone();
        ancestors.push(self.node.clone());
        Self {
            node: StyleNode {
                id: id.into(),
                role: role.into(),
                classes: classes.to_vec(),
                states: state.split_whitespace().map(str::to_owned).collect(),
                attributes: BTreeMap::new(),
            },
            ancestors,
        }
    }
}
impl Region {
    pub fn style_target(&self, layout: &str, state: &str) -> StyleTarget {
        StyleTarget {
            node: StyleNode {
                id: self.id.clone(),
                role: self.role.clone(),
                classes: self.classes.clone(),
                states: state.split_whitespace().map(str::to_owned).collect(),
                attributes: BTreeMap::from([("layout".into(), layout.into())]),
            },
            ancestors: vec![StyleNode {
                id: format!("layout:{layout}"),
                role: "presentation-root".into(),
                attributes: BTreeMap::from([("layout".into(), layout.into())]),
                ..Default::default()
            }],
        }
    }
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            schema_version: 1,
            variables: BTreeMap::new(),
            rules: vec![],
        }
    }
}
#[derive(Clone, Debug)]
enum Condition {
    Id(String),
    Role(String),
    Class(String),
    Attribute(String, Option<(bool, String)>),
    State(String),
    Root,
}
#[derive(Clone, Copy, Debug)]
enum Combinator {
    Descendant,
    Child,
}
#[derive(Clone, Debug)]
struct Selector {
    compounds: Vec<Vec<Condition>>,
    combinators: Vec<Combinator>,
    specificity: (usize, usize, usize),
}
type SelectorCache = Mutex<BTreeMap<String, Arc<Vec<Selector>>>>;
fn compiled_selectors(source: &str) -> Result<Arc<Vec<Selector>>> {
    static CACHE: OnceLock<SelectorCache> = OnceLock::new();
    if source.len() > 16_384 {
        return Err(invalid("Selector list exceeds 16384 bytes"));
    }
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Ok(entries) = cache.lock()
        && let Some(selectors) = entries.get(source)
    {
        return Ok(selectors.clone());
    }
    let parts = split_top_level(source, ',')?;
    if parts.len() > 64 {
        return Err(invalid("Selector list exceeds 64 alternatives"));
    }
    let selectors = Arc::new(
        parts
            .into_iter()
            .map(parse_selector)
            .collect::<Result<Vec<_>>>()?,
    );
    if let Ok(mut entries) = cache.lock() {
        if entries.len() >= 128
            && let Some(oldest) = entries.keys().next().cloned()
        {
            entries.remove(&oldest);
        }
        entries.insert(source.into(), selectors.clone());
    }
    Ok(selectors)
}
fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(message.into())
}
fn split_top_level(source: &str, delimiter: char) -> Result<Vec<&str>> {
    let mut result = vec![];
    let mut depth = 0i32;
    let mut quote = None;
    let mut escaped = false;
    let mut start = 0;
    for (index, c) in source.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        if matches!(c, '\'' | '"') {
            quote = Some(c);
            continue;
        }
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => {
                depth -= 1;
                if depth < 0 {
                    return Err(invalid("Unbalanced style expression"));
                }
            }
            _ => {}
        }
        if c == delimiter && depth == 0 {
            result.push(&source[start..index]);
            start = index + c.len_utf8();
        }
    }
    if depth != 0 || quote.is_some() || escaped {
        return Err(invalid("Unclosed style expression"));
    }
    result.push(&source[start..]);
    Ok(result)
}
fn identifier(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_')
}
fn parse_selector(source: &str) -> Result<Selector> {
    let mut chars = source.trim().char_indices().peekable();
    let mut compounds = vec![];
    let mut combinators = vec![];
    let mut specificity = (0, 0, 0);
    while chars.peek().is_some() {
        let mut compound = vec![];
        let mut type_selector = false;
        while let Some(&(index, c)) = chars.peek() {
            if c.is_whitespace() || c == '>' {
                break;
            }
            chars.next();
            match c {
                '*' => {
                    if type_selector || !compound.is_empty() {
                        return Err(invalid(format!(
                            "Invalid universal selector position in {source:?}"
                        )));
                    }
                    type_selector = true;
                }
                '#' | '.' | ':' => {
                    let start = chars.peek().map(|&(i, _)| i).unwrap_or(source.trim().len());
                    let mut end = start;
                    while let Some(&(i, next)) = chars.peek() {
                        if !identifier(next) {
                            break;
                        }
                        end = i + next.len_utf8();
                        chars.next();
                    }
                    if end == start {
                        return Err(invalid(format!("Missing selector name in {source:?}")));
                    }
                    let name = source.trim()[start..end].to_owned();
                    match c {
                        '#' => {
                            compound.push(Condition::Id(name));
                            specificity.0 += 1;
                        }
                        '.' => {
                            compound.push(Condition::Class(name));
                            specificity.1 += 1;
                        }
                        _ if name == "root" => {
                            compound.push(Condition::Root);
                            specificity.1 += 1;
                        }
                        _ => {
                            compound.push(Condition::State(name));
                            specificity.1 += 1;
                        }
                    }
                }
                '[' => {
                    let start = index + 1;
                    let mut quote = None;
                    let mut end = None;
                    for (i, next) in chars.by_ref() {
                        if let Some(q) = quote {
                            if next == q {
                                quote = None;
                            }
                        } else if matches!(next, '\'' | '"') {
                            quote = Some(next);
                        } else if next == ']' {
                            end = Some(i);
                            break;
                        }
                    }
                    let end =
                        end.ok_or_else(|| invalid(format!("Unclosed attribute in {source:?}")))?;
                    let attribute = source.trim()[start..end].trim();
                    let (key, comparison) = if let Some((key, value)) = attribute.split_once("~=") {
                        (key, Some((true, value)))
                    } else if let Some((key, value)) = attribute.split_once('=') {
                        (key, Some((false, value)))
                    } else {
                        (attribute, None)
                    };
                    if key.trim().is_empty() || !key.trim().chars().all(identifier) {
                        return Err(invalid(format!("Invalid selector attribute {attribute:?}")));
                    }
                    let comparison = comparison.map(|(tokens, value)| {
                        (tokens, value.trim().trim_matches(['\'', '"']).to_owned())
                    });
                    if comparison
                        .as_ref()
                        .is_some_and(|(_, value)| value.is_empty())
                    {
                        return Err(invalid("Empty selector attribute value"));
                    }
                    compound.push(Condition::Attribute(key.trim().into(), comparison));
                    specificity.1 += 1;
                }
                _ if identifier(c) => {
                    if type_selector || !compound.is_empty() {
                        return Err(invalid(format!(
                            "A role selector must begin its compound in {source:?}"
                        )));
                    }
                    type_selector = true;
                    let mut end = index + c.len_utf8();
                    while let Some(&(i, next)) = chars.peek() {
                        if !identifier(next) {
                            break;
                        }
                        end = i + next.len_utf8();
                        chars.next();
                    }
                    compound.push(Condition::Role(source.trim()[index..end].into()));
                    specificity.2 += 1;
                }
                _ => {
                    return Err(invalid(format!(
                        "Unsupported selector character {c:?} in {source:?}"
                    )));
                }
            }
        }
        if compound.is_empty() && !type_selector {
            return Err(invalid(format!("Empty selector compound in {source:?}")));
        }
        compounds.push(compound);
        let mut whitespace = false;
        while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
            whitespace = true;
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }
        let combinator = if chars.peek().is_some_and(|(_, c)| *c == '>') {
            chars.next();
            while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
                chars.next();
            }
            Combinator::Child
        } else if whitespace {
            Combinator::Descendant
        } else {
            return Err(invalid(format!(
                "Invalid selector combinator in {source:?}"
            )));
        };
        if chars.peek().is_none() {
            return Err(invalid(format!(
                "Selector ends with a combinator: {source:?}"
            )));
        }
        combinators.push(combinator);
    }
    if compounds.is_empty() {
        return Err(invalid("Empty style selector"));
    }
    Ok(Selector {
        compounds,
        combinators,
        specificity,
    })
}
fn condition_matches(condition: &Condition, node: &StyleNode, root: bool) -> bool {
    let value = |key: &str| match key {
        "id" => Some(node.id.clone()),
        "role" => Some(node.role.clone()),
        "class" => Some(node.classes.join(" ")),
        "state" => Some(node.states.join(" ")),
        _ => node.attributes.get(key).cloned(),
    };
    match condition {
        Condition::Id(id) => &node.id == id,
        Condition::Role(role) => &node.role == role,
        Condition::Class(class) => node.classes.contains(class),
        Condition::State(state) => node.states.contains(state),
        Condition::Root => root,
        Condition::Attribute(key, comparison) => {
            value(key).is_some_and(|actual| match comparison {
                None => true,
                Some((tokens, expected)) => {
                    if *tokens || key == "state" {
                        actual.split_whitespace().any(|part| part == expected)
                    } else {
                        actual == *expected
                    }
                }
            })
        }
    }
}
impl Selector {
    fn matches(&self, path: &[&StyleNode]) -> bool {
        fn walk(selector: &Selector, path: &[&StyleNode], compound: usize, index: usize) -> bool {
            if !selector.compounds[compound]
                .iter()
                .all(|c| condition_matches(c, path[index], index == 0))
            {
                return false;
            }
            if compound == 0 {
                return true;
            }
            if index == 0 {
                return false;
            }
            match selector.combinators[compound - 1] {
                Combinator::Child => walk(selector, path, compound - 1, index - 1),
                Combinator::Descendant => (0..index)
                    .rev()
                    .any(|ancestor| walk(selector, path, compound - 1, ancestor)),
            }
        }
        !path.is_empty() && walk(self, path, self.compounds.len() - 1, path.len() - 1)
    }
}
fn strip_comments(source: &str) -> Result<String> {
    let mut out = String::new();
    let mut remaining = source;
    while let Some(start) = remaining.find("/*") {
        out.push_str(&remaining[..start]);
        let rest = &remaining[start + 2..];
        let end = rest
            .find("*/")
            .ok_or_else(|| invalid("Unclosed CSS comment"))?;
        out.push(' ');
        remaining = &rest[end + 2..];
    }
    out.push_str(remaining);
    Ok(out)
}
fn resolve_variables(
    value: &str,
    variables: &BTreeMap<String, String>,
    stack: &mut Vec<String>,
) -> Result<String> {
    resolve_variables_depth(value, variables, stack, 0)
}
fn resolve_variables_depth(
    value: &str,
    variables: &BTreeMap<String, String>,
    stack: &mut Vec<String>,
    depth: usize,
) -> Result<String> {
    if stack.len() > 64 || depth > 64 {
        return Err(invalid("Style variable expansion exceeds 64 levels"));
    }
    let mut out = String::new();
    let mut remaining = value;
    while let Some(start) = remaining.find("var(") {
        out.push_str(&remaining[..start]);
        let mut depth = 1usize;
        let mut end = None;
        for (index, c) in remaining[start + 4..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(start + 4 + index);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = end.ok_or_else(|| invalid(format!("Unclosed variable expression {value:?}")))?;
        let arguments = split_top_level(&remaining[start + 4..end], ',')?;
        let name = arguments[0].trim();
        if !name.starts_with("--") || !name.chars().all(identifier) {
            return Err(invalid(format!("Invalid custom property name {name:?}")));
        }
        let replacement = if stack.iter().any(|active| active == name) {
            if arguments.len() > 1 {
                resolve_variables_depth(&arguments[1..].join(","), variables, stack, depth + 1)?
            } else {
                return Err(invalid(format!(
                    "Cyclic style variables: {} -> {name}",
                    stack.join(" -> ")
                )));
            }
        } else if let Some(replacement) = variables.get(name) {
            stack.push(name.into());
            let resolved = resolve_variables_depth(replacement, variables, stack, depth + 1);
            stack.pop();
            match resolved {
                Ok(value) => value,
                Err(_) if arguments.len() > 1 => {
                    resolve_variables_depth(&arguments[1..].join(","), variables, stack, depth + 1)?
                }
                Err(error) => return Err(error),
            }
        } else if arguments.len() > 1 {
            resolve_variables_depth(&arguments[1..].join(","), variables, stack, depth + 1)?
        } else {
            return Err(invalid(format!("Undefined style variable {name}")));
        };
        out.push_str(&replacement);
        if out.len() > 1_048_576 {
            return Err(invalid("Style variable expansion exceeds 1 MiB"));
        }
        remaining = &remaining[end + 1..];
    }
    out.push_str(remaining);
    Ok(out)
}
pub(super) fn length(value: &str, reference: f32) -> Option<f32> {
    let value = value.trim();
    let result = if let Some(percent) = value.strip_suffix('%') {
        percent.parse::<f32>().ok()? * reference / 100.
    } else {
        value
            .strip_suffix("px")
            .unwrap_or(value)
            .parse::<f32>()
            .ok()?
    };
    result.is_finite().then_some(result)
}
pub(super) fn insets(value: &str, reference: f32) -> Option<[f32; 4]> {
    let values: Option<Vec<_>> = value
        .split_whitespace()
        .map(|v| length(v, reference))
        .collect();
    match values?.as_slice() {
        [a] => Some([*a; 4]),
        [a, b] => Some([*a, *b, *a, *b]),
        [a, b, c] => Some([*a, *b, *c, *b]),
        [a, b, c, d] => Some([*a, *b, *c, *d]),
        _ => None,
    }
}
fn validate_property_name(name: &str) -> Result<()> {
    if name.starts_with("--") {
        if name.len() < 3 || !name.chars().all(identifier) {
            return Err(invalid(format!("Invalid style variable {name:?}")));
        }
    } else if !SUPPORTED_STYLE_PROPERTIES.contains(&name) && !name.starts_with("renderer-") {
        return Err(invalid(format!(
            "Unsupported style property {name:?}; renderer extensions require a renderer- prefix"
        )));
    }
    Ok(())
}
fn validate_variable_syntax(value: &str) -> Result<()> {
    split_top_level(value, ';')?;
    let mut remainder = value;
    while let Some(start) = remainder.find("var(") {
        let mut depth = 1usize;
        let mut end = None;
        for (index, c) in remainder[start + 4..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(start + 4 + index);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = end.ok_or_else(|| invalid(format!("Unclosed variable expression {value:?}")))?;
        let args = split_top_level(&remainder[start + 4..end], ',')?;
        validate_property_name(args[0].trim())?;
        if !args[0].trim().starts_with("--") {
            return Err(invalid(
                "var() requires a custom property name beginning --",
            ));
        }
        // Continue inside the expression so nested fallback syntax is checked too.
        remainder = &remainder[start + 4..];
    }
    Ok(())
}
fn validate_property(name: &str, value: &str) -> Result<()> {
    validate_property_name(name)?;
    if name.starts_with("--") || name.starts_with("renderer-") {
        return Ok(());
    }
    if !SUPPORTED_STYLE_PROPERTIES.contains(&name) {
        return Err(invalid(format!(
            "Unsupported style property {name:?}; renderer extensions require a renderer- prefix"
        )));
    }
    if value.is_empty() {
        return Err(invalid(format!("Empty value for {name}")));
    }
    if matches!(value, "inherit" | "initial" | "unset") {
        return Ok(());
    }
    if matches!(
        name,
        "color"
            | "background"
            | "accent-color"
            | "border-color"
            | "selection-color"
            | "preview-color"
            | "comparison-color"
            | "reference-curve-color"
            | "explode-line-color"
            | "inspection-first-color"
            | "inspection-second-color"
            | "drawing-active"
            | "drawing-item"
            | "drawing-preview"
            | "drawing-section"
            | "drawing-view"
    ) && !valid_color(value)
    {
        return Err(invalid(format!(
            "{name} requires #RGB, #RGBA, #RRGGBB, #RRGGBBAA, transparent, or a supported basic color name; received {value:?}"
        )));
    }
    if name == "border" && !matches!(value, "none" | "0") {
        let parts: Vec<_> = value.split_whitespace().collect();
        if !matches!(parts.as_slice(),[width,"solid",color] if length(width,100.).is_some_and(|width|width>=0.) && valid_color(color))
        {
            return Err(invalid(format!(
                "border requires '<width> solid <color>' or none; received {value:?}"
            )));
        }
    }
    let length_property = matches!(
        name,
        "font-size"
            | "row-gap"
            | "column-gap"
            | "border-width"
            | "stroke-width"
            | "size"
            | "hit-size"
            | "hit-width"
            | "hit-height"
            | "icon-size"
            | "button-width"
            | "button-height"
            | "handle-size"
            | "min-target-size"
            | "width"
            | "height"
            | "min-width"
            | "max-width"
            | "min-height"
            | "max-height"
            | "padding-top"
            | "padding-right"
            | "padding-bottom"
            | "padding-left"
            | "border-radius"
    );
    let auto = value == "auto" && matches!(name, "width" | "height");
    if length_property && !auto && length(value, 100.).is_none_or(|v| v < 0.) {
        return Err(invalid(format!(
            "{name} requires a nonnegative px or % length; received {value:?}"
        )));
    }
    if matches!(name, "padding") && insets(value, 100.).is_none_or(|v| v.iter().any(|v| *v < 0.)) {
        return Err(invalid(format!("Invalid {name} shorthand {value:?}")));
    }
    if name == "gap" {
        let values: Vec<_> = value.split_whitespace().collect();
        if !(1..=2).contains(&values.len())
            || values
                .iter()
                .any(|value| length(value, 100.).is_none_or(|value| value < 0.))
        {
            return Err(invalid(format!(
                "gap requires one or two nonnegative lengths; received {value:?}"
            )));
        }
    }
    if matches!(name, "top" | "right" | "bottom" | "left")
        && value != "auto"
        && length(value, 100.).is_none()
    {
        return Err(invalid(format!("Invalid position {name}: {value}")));
    }
    if matches!(
        name,
        "opacity" | "selection-opacity" | "ghost-opacity" | "construction-opacity"
    ) && value
        .parse::<f32>()
        .ok()
        .is_none_or(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
    {
        return Err(invalid(format!("{name} requires a value from 0 to 1")));
    }
    if name == "flex-grow"
        && value
            .parse::<f32>()
            .ok()
            .is_none_or(|v| !v.is_finite() || v < 0.)
    {
        return Err(invalid("flex-grow requires a nonnegative number"));
    }
    let allowed: Option<&[&str]> = match name {
        "display" => Some(&["none", "block", "flex"]),
        "visibility" => Some(&["visible", "hidden"]),
        "flex-direction" => Some(&["row", "column"]),
        "align-items" => Some(&["stretch", "start", "end", "center"]),
        "justify-content" => Some(&["start", "end", "center", "space-between"]),
        "position" => Some(&["relative", "absolute"]),
        "overflow" => Some(&["clip", "hidden", "scroll", "auto"]),
        "text-align" => Some(&["left", "right", "center"]),
        "shape" => Some(&["rectangle", "rounded", "circle"]),
        "anchor" => Some(&[
            "top-left",
            "top",
            "top-right",
            "left",
            "center",
            "right",
            "bottom-left",
            "bottom",
            "bottom-right",
        ]),
        _ => None,
    };
    if allowed.is_some_and(|allowed| !allowed.contains(&value)) {
        return Err(invalid(format!("Unsupported {name} value {value:?}")));
    }
    Ok(())
}
fn valid_color(value: &str) -> bool {
    value.strip_prefix('#').is_some_and(|hex| {
        matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
    }) || matches!(
        value.to_ascii_lowercase().as_str(),
        "transparent"
            | "black"
            | "white"
            | "gray"
            | "grey"
            | "red"
            | "green"
            | "blue"
            | "orange"
            | "yellow"
            | "cyan"
            | "magenta"
    )
}
impl Theme {
    pub fn load(path: &Path) -> Result<Self> {
        let source = std::fs::read_to_string(path)?;
        let theme = if path.extension().is_some_and(|extension| extension == "css") {
            Self::from_css(&source)?
        } else {
            serde_json::from_str(&source)?
        };
        theme.validate()?;
        Ok(theme)
    }
    pub fn parse_css(source: &str) -> Result<Self> {
        let source = strip_comments(source)?;
        let mut theme = Self::default();
        let mut remainder = source.as_str();
        while let Some(open) = remainder.find('{') {
            let selector = remainder[..open].trim();
            let rest = &remainder[open + 1..];
            let end = rest
                .find('}')
                .ok_or_else(|| invalid("Unclosed CSS block"))?;
            let mut declarations = 0usize;
            for declaration in split_top_level(&rest[..end], ';')?
                .into_iter()
                .filter(|value| !value.trim().is_empty())
            {
                let (name, value) = declaration
                    .split_once(':')
                    .ok_or_else(|| invalid(format!("Expected CSS declaration in {selector:?}")))?;
                declarations += 1;
                theme.rules.push(StyleRule {
                    selector: selector.into(),
                    properties: BTreeMap::from([(name.trim().to_owned(), value.trim().to_owned())]),
                });
            }
            if declarations == 0 {
                theme.rules.push(StyleRule {
                    selector: selector.into(),
                    properties: BTreeMap::new(),
                });
            }
            remainder = &rest[end + 1..];
        }
        if !remainder.trim().is_empty() {
            return Err(invalid("Incomplete CSS rule"));
        }
        Ok(theme)
    }
    pub fn from_css(source: &str) -> Result<Self> {
        let theme = Self::parse_css(source)?;
        theme.validate()?;
        Ok(theme)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(invalid(format!(
                "Unsupported theme schema_version {}; expected 1",
                self.schema_version
            )));
        }
        if self.rules.len() > 16_384 || self.variables.len() > 4096 {
            return Err(invalid("Theme exceeds rule or variable limits"));
        }
        let variables = &self.variables;
        for (name, value) in variables {
            if !name.starts_with("--") || name.len() < 3 || !name.chars().all(identifier) {
                return Err(invalid(format!("Invalid style variable {name:?}")));
            }
            validate_variable_syntax(value)?;
            resolve_variables(value, variables, &mut vec![name.clone()])
                .map_err(|error| invalid(format!("Variable {name}: {error}")))?;
        }
        for (index, rule) in self.rules.iter().enumerate() {
            compiled_selectors(&rule.selector).map_err(|error| {
                invalid(format!(
                    "Theme rule {} ({:?}): {error}",
                    index + 1,
                    rule.selector
                ))
            })?;
            for (name, value) in &rule.properties {
                validate_variable_syntax(important_value(value).0)?;
                validate_property_name(name)?;
                if important_value(value).0.contains("var(") {
                    continue;
                }
                validate_property(name, important_value(value).0).map_err(|error| {
                    invalid(format!(
                        "Theme rule {} declaration {name}: {error}",
                        index + 1
                    ))
                })?;
            }
        }
        Ok(())
    }
    pub fn validate_renderer_extensions(&self, supported: &[&str]) -> Result<()> {
        self.validate()?;
        for (index, rule) in self.rules.iter().enumerate() {
            for name in rule
                .properties
                .keys()
                .filter(|name| name.starts_with("renderer-"))
            {
                if !supported.contains(&name.as_str()) {
                    return Err(invalid(format!(
                        "Theme rule {} requires renderer property {name:?}, which this adapter does not support",
                        index + 1
                    )));
                }
            }
        }
        Ok(())
    }
    pub fn properties(
        &self,
        region: &Region,
        layout: &str,
        state: &str,
    ) -> BTreeMap<String, String> {
        self.properties_for(&region.style_target(layout, state))
    }
    pub fn properties_for(&self, target: &StyleTarget) -> BTreeMap<String, String> {
        self.compute_properties(target, false).unwrap_or_default()
    }
    pub fn properties_checked_for(&self, target: &StyleTarget) -> Result<BTreeMap<String, String>> {
        self.compute_properties(target, true)
    }
    fn compute_properties(
        &self,
        target: &StyleTarget,
        checked: bool,
    ) -> Result<BTreeMap<String, String>> {
        let parsed: Vec<_> = self
            .rules
            .iter()
            .enumerate()
            .filter_map(|(index, rule)| {
                compiled_selectors(&rule.selector)
                    .ok()
                    .map(|selectors| (index, selectors, &rule.properties))
            })
            .collect();
        let path: Vec<_> = target
            .ancestors
            .iter()
            .chain(std::iter::once(&target.node))
            .collect();
        let mut parent_computed = BTreeMap::<String, String>::new();
        let mut variables = self.variables.clone();
        variables = resolve_variable_map(&variables, checked, "global variables")?;
        for depth in 0..path.len() {
            let mut cascade =
                BTreeMap::<String, ((bool, (usize, usize, usize), usize), String)>::new();
            for (index, selectors, properties) in &parsed {
                for selector in selectors.iter() {
                    if !selector.matches(&path[..=depth]) {
                        continue;
                    }
                    for (name, value) in *properties {
                        let (value, important) = important_value(value);
                        let priority = (important, selector.specificity, *index);
                        if cascade
                            .get(name)
                            .is_none_or(|(previous, _)| priority >= *previous)
                        {
                            cascade.insert(name.clone(), (priority, value.into()));
                        }
                    }
                }
            }
            for (name, (_, value)) in &cascade {
                if name.starts_with("--") {
                    match value.as_str() {
                        "inherit" | "unset" => {}
                        "initial" => {
                            variables.remove(name);
                        }
                        _ => {
                            variables.insert(name.clone(), value.clone());
                        }
                    }
                }
            }
            let candidates = variables.clone();
            variables = resolve_variable_map(&candidates, checked, &path[depth].id)?;
            expand_shorthands(&mut cascade, &variables, checked, &path[depth].id)?;
            let mut computed: BTreeMap<_, _> = parent_computed
                .iter()
                .filter(|(name, _)| INHERITED.contains(&name.as_str()))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            for (name, (_, value)) in cascade {
                if name.starts_with("--") {
                    continue;
                }
                match value.as_str() {
                    "inherit" => {
                        if let Some(value) = parent_computed.get(&name) {
                            computed.insert(name, value.clone());
                        } else {
                            computed.remove(&name);
                        }
                    }
                    "initial" => {
                        computed.remove(&name);
                    }
                    "unset" if !INHERITED.contains(&name.as_str()) => {
                        computed.remove(&name);
                    }
                    "unset" => {}
                    _ => {
                        match resolve_variables(&value, &variables, &mut vec![]).and_then(|value| {
                            validate_property(&name, &value)?;
                            Ok(value)
                        }) {
                            Ok(value) => {
                                computed.insert(name, value);
                            }
                            Err(error) if checked => {
                                return Err(invalid(format!(
                                    "Style target {} declaration {name}: {error}",
                                    path[depth].id
                                )));
                            }
                            Err(_) => {}
                        }
                    }
                }
            }
            canonical_shorthands(&mut computed);
            if depth + 1 == path.len() {
                computed.extend(variables);
                return Ok(computed);
            }
            parent_computed = computed;
        }
        Ok(BTreeMap::new())
    }
    pub fn selectors(&self) -> BTreeSet<String> {
        self.rules
            .iter()
            .map(|rule| rule.selector.clone())
            .collect()
    }
}
fn important_value(value: &str) -> (&str, bool) {
    value
        .trim()
        .strip_suffix("!important")
        .map(|value| (value.trim(), true))
        .unwrap_or((value.trim(), false))
}
type Cascade = BTreeMap<String, ((bool, (usize, usize, usize), usize), String)>;
fn resolve_variable_map(
    values: &BTreeMap<String, String>,
    checked: bool,
    target: &str,
) -> Result<BTreeMap<String, String>> {
    let mut resolved = BTreeMap::new();
    for (name, value) in values {
        match resolve_variables(value, values, &mut vec![name.clone()]) {
            Ok(value) => {
                resolved.insert(name.clone(), value);
            }
            Err(error) if checked => {
                return Err(invalid(format!(
                    "Style target {target} custom property {name}: {error}"
                )));
            }
            Err(_) => {}
        }
    }
    Ok(resolved)
}
fn expand_shorthands(
    cascade: &mut Cascade,
    variables: &BTreeMap<String, String>,
    checked: bool,
    target: &str,
) -> Result<()> {
    for name in ["padding", "border", "gap"] {
        let Some((priority, raw)) = cascade.get(name).cloned() else {
            continue;
        };
        let value = match resolve_variables(&raw, variables, &mut vec![]) {
            Ok(value) => value,
            Err(error) if checked => {
                return Err(invalid(format!(
                    "Style target {target} declaration {name}: {error}"
                )));
            }
            Err(_) => {
                cascade.remove(name);
                continue;
            }
        };
        let keywords = matches!(value.as_str(), "inherit" | "initial" | "unset");
        let parts: Vec<_> = value.split_whitespace().map(str::to_owned).collect();
        let expanded: Vec<(&str, String)> = match name {
            "padding" => {
                let values = if keywords {
                    Some([value.clone(), value.clone(), value.clone(), value.clone()])
                } else {
                    match parts.as_slice() {
                        [a] => Some([a.clone(), a.clone(), a.clone(), a.clone()]),
                        [a, b] => Some([a.clone(), b.clone(), a.clone(), b.clone()]),
                        [a, b, c] => Some([a.clone(), b.clone(), c.clone(), b.clone()]),
                        [a, b, c, d] => Some([a.clone(), b.clone(), c.clone(), d.clone()]),
                        _ => None,
                    }
                };
                values
                    .map(|values| {
                        [
                            "padding-top",
                            "padding-right",
                            "padding-bottom",
                            "padding-left",
                        ]
                        .into_iter()
                        .zip(values)
                        .collect()
                    })
                    .unwrap_or_default()
            }
            "border" if keywords => vec![
                ("border-width", value.clone()),
                ("border-color", value.clone()),
            ],
            "border" if matches!(value.as_str(), "none" | "0") => vec![
                ("border-width", "0px".into()),
                ("border-color", "transparent".into()),
            ],
            "border" => match parts.as_slice() {
                [width, _, color] => vec![
                    ("border-width", width.clone()),
                    ("border-color", color.clone()),
                ],
                _ => vec![],
            },
            "gap" if keywords => vec![("row-gap", value.clone()), ("column-gap", value.clone())],
            "gap" => match parts.as_slice() {
                [one] => vec![("row-gap", one.clone()), ("column-gap", one.clone())],
                [row, column] => vec![("row-gap", row.clone()), ("column-gap", column.clone())],
                _ => vec![],
            },
            _ => vec![],
        };
        for (name, value) in expanded {
            if cascade
                .get(name)
                .is_none_or(|(previous, _)| priority > *previous)
            {
                cascade.insert(name.into(), (priority, value));
            }
        }
    }
    Ok(())
}
fn canonical_shorthands(properties: &mut BTreeMap<String, String>) {
    let padding: Option<Vec<_>> = [
        "padding-top",
        "padding-right",
        "padding-bottom",
        "padding-left",
    ]
    .into_iter()
    .map(|name| properties.get(name).cloned())
    .collect();
    if let Some(padding) = padding {
        properties.insert("padding".into(), padding.join(" "));
    }
    if properties.contains_key("border")
        && let (Some(width), Some(color)) = (
            properties.get("border-width"),
            properties.get("border-color"),
        )
    {
        properties.insert("border".into(), format!("{width} solid {color}"));
    }
}
